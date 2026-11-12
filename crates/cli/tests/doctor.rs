//! M63-15 — `dh doctor`, end to end against a stub instance.
//!
//! The unit tests in `doctor.rs` prove the arithmetic (signed skew, API line, which statuses move the exit code). What
//! only a spawned binary can get wrong:
//!
//! 1. **The report is produced from a real HTTP exchange**, including the server's `Date` header — a doctor that reads a
//!    header the server never sent would report a clean clock for every broken host.
//! 2. **A warning does not fail a preflight.** `dh doctor` is run by scripts before other verbs; exit `1` must mean "this
//!    is why it is broken", not "something is slightly off".
//! 3. **The profile is not consulted when `--base-url` is given**, and the helper strips `DARKHOLD_*` from the environment
//!    so a developer's own config cannot turn this test green.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::Command;

const PASSWORD_ENV: [&str; 5] = [
    "DARKHOLD_CONFIG",
    "DARKHOLD_PROFILE",
    "DARKHOLD_BASE_URL",
    "DARKHOLD_TOKEN",
    "DARKHOLD_COOKIE",
];

/// A one-shot HTTP server that answers `/api/v1/meta` and `/api/v1/me/credential`, then exits with the accept loop.
fn stub(version: &'static str, date_skew: Option<i64>) -> (u16, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a stub port");
    let port = listener.local_addr().expect("the bound address").port();
    let handle = std::thread::spawn(move || {
        // Accept until the client stops asking, then leave: the binary makes one request without a token and two with
        // one, so a fixed count would either hang the join or cut the second request off.
        let _ = listener.set_nonblocking(true);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            let (mut stream, _) = match listener.accept() {
                Ok(pair) => pair,
                Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    continue;
                }
                Err(_) => return,
            };
            let _ = stream.set_nonblocking(false);
            let mut reader = BufReader::new(stream.try_clone().expect("clone the stream"));
            let mut request_line = String::new();
            let _ = reader.read_line(&mut request_line);
            // Drain the headers so the client's write finishes.
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
            }

            let body = if request_line.contains("/api/v1/me/credential") {
                r#"{"scopes":["content.read","library.read"]}"#.to_owned()
            } else {
                format!(r#"{{"version":"{version}"}}"#)
            };
            let date = match date_skew {
                // A server whose clock is two hours behind the test's, which is what a mis-set VM looks like.
                Some(skew) => {
                    let at = time::OffsetDateTime::now_utc() - time::Duration::seconds(skew);
                    format!(
                        "\r\nDate: {}",
                        at.format(&time::format_description::well_known::Rfc2822)
                            .expect("format a date")
                    )
                }
                None => String::new(),
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}{date}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });
    (port, handle)
}

fn doctor(port: u16, extra: &[&str]) -> (i32, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_dh"));
    for name in PASSWORD_ENV {
        command.env_remove(name);
    }
    command
        .arg("doctor")
        .arg("--base-url")
        .arg(format!("http://127.0.0.1:{port}"))
        .arg("--json");
    for arg in extra {
        command.arg(arg);
    }
    let output = command.output().expect("run dh doctor");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned()
            + &String::from_utf8_lossy(&output.stderr),
    )
}

#[test]
fn a_matching_instance_reports_clean_and_exits_zero() {
    // A real instance sends `Date`; without one the doctor can only say it cannot rule skew out.
    let (port, handle) = stub("0.1.0+abc123", Some(0));
    let (code, output) = doctor(port, &[]);
    handle.join().expect("the stub thread ends");

    let report: serde_json::Value = serde_json::from_str(&output).expect("a JSON report");
    let checks = report["checks"].as_array().expect("checks");
    let find = |name: &str| {
        checks
            .iter()
            .find(|check| check["name"] == name)
            .unwrap_or_else(|| panic!("{name} was measured: {output}"))
            .clone()
    };

    assert_eq!(
        find("instance")["status"],
        "ok",
        "the version is read from the instance, not assumed: {output}"
    );
    assert_eq!(
        find("version")["status"],
        "ok",
        "0.1.0+abc123 is this binary's API line: {output}"
    );
    assert_eq!(
        find("clock")["status"], "ok",
        "a server whose clock agrees reads clean, and the measurement comes from its own Date header: {output}"
    );
    assert_eq!(
        find("token")["status"],
        "warn",
        "no token is a warning, never a failure: {output}"
    );
    assert_eq!(report["failed"], 0);
    assert_eq!(code, 0, "a warning must not fail a preflight: {output}");
}

#[test]
fn a_server_two_hours_behind_is_reported_with_a_fix() {
    let (port, handle) = stub("0.1.0+abc123", Some(7_200));
    let (code, output) = doctor(port, &[]);
    handle.join().expect("the stub thread ends");

    let report: serde_json::Value = serde_json::from_str(&output).expect("a JSON report");
    let clock = report["checks"]
        .as_array()
        .expect("checks")
        .iter()
        .find(|check| check["name"] == "clock")
        .expect("the clock was measured");
    assert_eq!(clock["status"], "warn", "{output}");
    assert!(
        clock["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("7200"),
        "the skew is a number, signed and in seconds: {output}"
    );
    assert!(
        clock["fix"].as_str().unwrap_or_default().contains("NTP"),
        "and it comes with one suggested fix: {output}"
    );
    assert_eq!(code, 0, "a skewed clock is a warning, not a failure");
}
