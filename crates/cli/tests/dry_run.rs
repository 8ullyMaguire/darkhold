//! `M63-21` — a mutating verb sends nothing without `--yes`, and `dh redact` scrubs a paste.
//!
//! These run the real binary against a server that **counts connections**, because the assertion that
//! matters is "no request was sent", and a printed preview alone would pass even if the write went
//! out behind it.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// A server that answers `{}` to everything and counts what it served.
fn counting_server() -> (u16, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let served = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&served);
    std::thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(stream) = incoming else { break };
            counter.fetch_add(1, Ordering::SeqCst);
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let mut line = String::new();
            while reader.read_line(&mut line).map(|n| n > 0).unwrap_or(false) {
                if line == "\r\n" {
                    break;
                }
                line.clear();
            }
            let body = "{}";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n{body}",
                body.len()
            );
            let mut stream = stream;
            let _ = stream.write_all(response.as_bytes());
        }
    });
    (port, served)
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn dh(port: u16, args: &[&str]) -> Run {
    let output = Command::new(env!("CARGO_BIN_EXE_dh"))
        .arg("--base-url")
        .arg(format!("http://127.0.0.1:{port}"))
        .args(args)
        .env("DARKHOLD_COOKIE", "darkhold_session=test")
        .env("DARKHOLD_TOKEN", "test-token")
        .stdin(Stdio::null())
        .output()
        .expect("run dh");
    Run {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

#[test]
fn withdrawing_a_surface_previews_and_sends_nothing() {
    let (port, served) = counting_server();
    let run = dh(port, &["module", "off", "arena"]);

    assert_eq!(run.code, 0, "a preview is a success: {}", run.stderr);
    assert_eq!(
        served.load(Ordering::SeqCst),
        0,
        "the point is that NO request was sent, not that one was quietly sent and ignored"
    );
    assert!(
        run.stdout.contains("would PATCH"),
        "the preview names the method and the door: {:?}",
        run.stdout
    );
    assert!(
        run.stdout.contains("/api/v1/modules/arena"),
        "and the URL: {:?}",
        run.stdout
    );
    assert!(
        run.stdout.contains("--yes"),
        "it says how to apply: {:?}",
        run.stdout
    );
}

#[test]
fn with_yes_the_patch_is_sent() {
    let (port, served) = counting_server();
    let run = dh(port, &["module", "off", "arena", "--yes"]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
    assert_eq!(served.load(Ordering::SeqCst), 1, "exactly one request");
    assert!(
        !run.stdout.contains("would "),
        "an applied change does not claim it is a preview: {:?}",
        run.stdout
    );
}

#[test]
fn marking_a_routing_complete_is_a_write_and_is_previewed() {
    let (port, served) = counting_server();
    let run = dh(
        port,
        &["supply", "complete", "00000000-0000-0000-0000-000000000001"],
    );
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(
        served.load(Ordering::SeqCst),
        0,
        "a payment is not a preview"
    );
    assert!(run.stdout.contains("would POST"), "{:?}", run.stdout);
}

#[test]
fn a_read_is_never_previewed() {
    // The guard is on the METHOD, so `supply routing` (a GET) must go straight through — a read that
    // demanded `--yes` would make the tool useless and train people to pass the flag by reflex.
    let (port, served) = counting_server();
    let run = dh(port, &["supply", "routing"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(served.load(Ordering::SeqCst), 1, "the read is sent");
    assert!(!run.stdout.contains("would "), "{:?}", run.stdout);
}

#[test]
fn apply_and_dry_run_together_are_a_usage_error() {
    let (port, served) = counting_server();
    let run = dh(port, &["module", "off", "arena", "--yes", "--dry-run"]);
    assert_eq!(
        run.code, 2,
        "there is no tie-break between apply and do-not-apply: {}",
        run.stderr
    );
    assert_eq!(served.load(Ordering::SeqCst), 0);
}

#[test]
fn redact_scrubs_ids_and_emails_from_a_piped_transcript() {
    let output = Command::new(env!("CARGO_BIN_EXE_dh"))
        .arg("redact")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            child.stdin.as_mut().expect("stdin").write_all(
                b"{\"pseud\":\"734911ed-41fc-4458-aa70-2c5734fb0bc1\",\"email\":\"a@b.co\"}",
            )?;
            child.wait_with_output()
        })
        .expect("run dh redact");

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, "{\"pseud\":\"uuid#1\",\"email\":\"email#1\"}");
    assert!(
        !stdout.contains("734911ed") && !stdout.contains("a@b.co"),
        "nothing identifiable survives: {stdout}"
    );
}

#[test]
fn redact_is_idempotent_because_a_paste_may_be_redacted_twice() {
    let once = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "echo 'a@b.co 00000000-0000-0000-0000-00000000000f' | {} redact",
            env!("CARGO_BIN_EXE_dh")
        ))
        .output()
        .expect("first pass");
    let first = String::from_utf8_lossy(&once.stdout).into_owned();
    let twice = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "printf '%s' '{first}' | {} redact",
            env!("CARGO_BIN_EXE_dh")
        ))
        .output()
        .expect("second pass");
    assert_eq!(
        String::from_utf8_lossy(&twice.stdout),
        first,
        "redacting a redacted transcript must not rename its placeholders"
    );
}
