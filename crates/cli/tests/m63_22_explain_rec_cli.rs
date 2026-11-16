//! M63-22 — `dh admin explain-rec` against a one-shot stub.
//!
//! What only the binary can get wrong: the query is encoded, the session cookie is actually sent, a null rank is
//! printed as a dash rather than a zero, and `--json` hands the server's bytes through unchanged.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::Command;

const BODY: &str = r#"{"account":"acct-1","work":"work-1","limit":200,"blended_position":2,"strategies":[{"name":"bandit","produced":1,"rank":1,"rrf":0.25},{"name":"cooccurrence","produced":0,"rank":null,"rrf":null}],"features":{"words":1000,"chapters":3,"kudos":4,"hits":5,"bookmarks":6,"comments":7,"rating":"teen","complete":true,"published_at":"2026-11-01T00:00:00Z","age_days":15.5,"tags":["naruto"]}}"#;

fn stub() -> (u16, std::thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a stub port");
    let port = listener.local_addr().expect("the bound address").port();
    let handle = std::thread::spawn(move || {
        let mut lines = Vec::new();
        let _ = listener.set_nonblocking(true);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while std::time::Instant::now() < deadline {
            let (mut stream, _) = match listener.accept() {
                Ok(pair) => pair,
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    continue;
                }
                Err(_) => return lines,
            };
            let _ = stream.set_nonblocking(false);
            let mut reader = BufReader::new(stream.try_clone().expect("clone the stream"));
            let mut request_line = String::new();
            let _ = reader.read_line(&mut request_line);
            lines.push(request_line.trim_end().to_owned());
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                lines.push(line.trim_end().to_owned());
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{BODY}",
                BODY.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
        lines
    });
    (port, handle)
}

fn dh(port: u16, json: bool) -> (i32, String, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_dh"));
    for name in [
        "DARKHOLD_CONFIG",
        "DARKHOLD_PROFILE",
        "DARKHOLD_BASE_URL",
        "DARKHOLD_TOKEN",
    ] {
        command.env_remove(name);
    }
    command
        .arg("admin")
        .arg("explain-rec")
        .arg("work-1")
        .arg("--account")
        .arg("acct-1")
        .arg("--base-url")
        .arg(format!("http://127.0.0.1:{port}"))
        .arg("--cookie")
        .arg("darkhold_session=operator");
    if json {
        command.arg("--json");
    }
    let output = command.output().expect("run dh");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn explain_rec_prints_a_table_with_a_dash_for_a_strategy_that_never_looked() {
    let (port, handle) = stub();
    let (code, stdout, stderr) = dh(port, false);
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(stdout.contains("bandit"), "{stdout}");
    assert!(stdout.contains("cooccurrence"), "{stdout}");
    assert!(stdout.contains("RANK"), "{stdout}");
    assert!(
        stdout.contains(" -"),
        "a missing rank must print as a dash: {stdout}"
    );
    assert!(stdout.contains("words=1000"), "{stdout}");
    let lines = handle.join().expect("stub thread");
    assert!(
        lines[0].contains("/api/v1/admin/discovery/explain-rec?account=acct-1&work=work-1"),
        "{}",
        lines[0]
    );
    assert!(
        lines.iter().any(|l| l
            .to_lowercase()
            .contains("cookie: darkhold_session=operator")),
        "{lines:?}"
    );
}

#[test]
fn json_prints_the_servers_body_byte_for_byte() {
    let (port, handle) = stub();
    let (code, stdout, stderr) = dh(port, true);
    assert_eq!(code, 0, "stderr={stderr}");
    assert_eq!(
        stdout.trim_end(),
        BODY,
        "--json must be a pipe, not a reformat"
    );
    let _ = handle.join();
}
