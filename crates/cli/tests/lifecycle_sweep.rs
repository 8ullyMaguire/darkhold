//! M45-76 Step 4.3: `dh lifecycle sweep` sends the operator door's request shape.
//!
//! The server here is a raw TCP listener so the test can see the JSON body that
//! was actually sent, not just the report the command printed.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

/// Serve one scripted response per connection and record the posted bodies.
fn scripted_server(script: &[(u16, &'static str)]) -> (u16, Arc<Mutex<Vec<String>>>) {
    let script = script.to_vec();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    std::thread::spawn(move || {
        let mut n = 0usize;
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else { break };
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let mut length = 0usize;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).map(|v| v == 0).unwrap_or(true) {
                    break;
                }
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0u8; length];
            if reader.read_exact(&mut body).is_ok() {
                sink.lock()
                    .expect("record")
                    .push(String::from_utf8_lossy(&body).into_owned());
            }
            let (status, payload) = *script.get(n).unwrap_or(script.last().expect("script"));
            let response = format!(
                "HTTP/1.1 {status} Status\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                payload.len()
            );
            let _ = stream.write_all(response.as_bytes());
            n += 1;
        }
    });
    (port, seen)
}

fn dh(port: u16, args: &[&str], cookie: bool) -> Run {
    let mut command = Command::new(env!("CARGO_BIN_EXE_dh"));
    command
        .arg("--base-url")
        .arg(format!("http://127.0.0.1:{port}"))
        .args(args)
        .stdin(Stdio::null());
    if cookie {
        command.env("DARKHOLD_COOKIE", "darkhold_session=test");
    } else {
        command.env_remove("DARKHOLD_COOKIE");
    }
    let output = command.output().expect("run dh");
    Run {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

#[test]
fn the_default_is_a_dry_run_and_apply_all_follows_the_cursor() {
    let page1 = r#"{"scanned":2,"abandoned":1,"revived":0,"concluded":0,"raced":0,"unmeasured":0,"next_cursor":"w1","done":false,"dry_run":false}"#;
    let page2 = r#"{"scanned":1,"abandoned":0,"revived":0,"concluded":0,"raced":0,"unmeasured":0,"next_cursor":null,"done":true,"dry_run":false}"#;

    let (port, seen) = scripted_server(&[(200, page2)]);
    let run = dh(port, &["lifecycle", "sweep", "--limit", "10"], true);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert!(run.stdout.contains("abandoned=0"), "{}", run.stdout);
    let bodies = seen.lock().expect("bodies").clone();
    assert_eq!(bodies.len(), 1);
    assert!(bodies[0].contains("\"dry_run\":true"), "{}", bodies[0]);
    assert!(bodies[0].contains("\"limit\":10"), "{}", bodies[0]);

    let (port, seen) = scripted_server(&[(200, page1), (200, page2)]);
    let run = dh(
        port,
        &["lifecycle", "sweep", "--apply", "--all", "--limit", "2"],
        true,
    );
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(run.stdout.lines().count(), 2, "{}", run.stdout);
    let bodies = seen.lock().expect("bodies").clone();
    assert_eq!(bodies.len(), 2);
    assert!(bodies[0].contains("\"dry_run\":false"), "{}", bodies[0]);
    assert!(bodies[1].contains("\"cursor\":\"w1\""), "{}", bodies[1]);
}

#[test]
fn a_missing_cookie_is_an_auth_error_and_a_refused_write_is_usage() {
    let ok = r#"{"scanned":0,"abandoned":0,"revived":0,"concluded":0,"raced":0,"unmeasured":0,"next_cursor":null,"done":true,"dry_run":true}"#;
    let (port, seen) = scripted_server(&[(200, ok)]);
    let run = dh(port, &["lifecycle", "sweep"], false);
    assert_eq!(
        run.code, 4,
        "missing session must be exit 4: {}",
        run.stdout
    );
    assert!(
        seen.lock().expect("bodies").is_empty(),
        "no request without a session"
    );

    let refused = r#"{"error":{"message":"automatic abandonment is disabled"}}"#;
    let (port, _) = scripted_server(&[(422, refused)]);
    let run = dh(port, &["lifecycle", "sweep", "--apply"], true);
    assert_eq!(run.code, 2, "a 422 is usage: {}", run.stdout);
}
