//! `M63-20` — a rate limit is waited out, and giving up is code `5` with the window named.
//!
//! A batch that dies at row 400 because of a `429` is a tooling bug, not a user error. These tests
//! drive the real binary against a server that answers a script of statuses, so they can prove three
//! things a unit test cannot: that the request is **retried**, that the caller's `stdout` stays
//! parseable while the wait is announced on `stderr`, and that the give-up code is `5`.
//!
//! `Retry-After: 0` keeps the suite fast. The jitter on top is capped at 250 ms, so two retries cost
//! well under a second — a sleeping test is a test people delete.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Serve one request per scripted status, in order, then keep answering the last one.
/// Returns the port and a counter of requests actually served.
fn scripted_server(
    script: &'static [(u16, &'static str)],
    retry_after: &'static str,
) -> (u16, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let served = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&served);
    std::thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(stream) = incoming else { break };
            let n = counter.fetch_add(1, Ordering::SeqCst);
            let (status, body) = script
                .get(n)
                .copied()
                .unwrap_or(*script.last().expect("script"));
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let mut line = String::new();
            while reader
                .read_line(&mut line)
                .map(|read| read > 0)
                .unwrap_or(false)
            {
                if line == "\r\n" {
                    break;
                }
                line.clear();
            }
            let response = format!(
                "HTTP/1.1 {status} Status\r\nContent-Type: application/json\r\n\
                 Retry-After: {retry_after}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let mut stream = stream;
            let _ = stream.write_all(response.as_bytes());
            if n + 1 >= script.len() {
                // Keep the listener alive for the remaining retries; the last status repeats.
            }
        }
    });
    (port, served)
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn dh(args: &[&str]) -> Run {
    let output = Command::new(env!("CARGO_BIN_EXE_dh"))
        .args(args)
        .env_remove("DARKHOLD_TOKEN")
        .env_remove("DARKHOLD_COOKIE")
        .output()
        .expect("run dh");
    Run {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn rec(port: u16, extra: &[&str]) -> Run {
    let url = format!("http://127.0.0.1:{port}");
    let mut args = vec!["--base-url".to_owned(), url, "rec".to_owned()];
    args.extend(extra.iter().map(|s| (*s).to_owned()));
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    dh(&refs)
}

#[test]
fn a_rate_limited_request_is_retried_and_the_wait_is_announced_on_stderr() {
    let (port, served) = scripted_server(
        &[(429, r#"{"error":{"message":"slow down"}}"#), (200, "{}")],
        "0",
    );

    let run = rec(port, &["--fandom", "naruto", "--json"]);

    assert_eq!(
        run.code, 0,
        "the second attempt succeeded: stderr={}",
        run.stderr
    );
    assert_eq!(
        served.load(Ordering::SeqCst),
        2,
        "the 429 must be retried exactly once before the success"
    );
    assert!(
        run.stderr.contains("rate-limited: waiting"),
        "the wait must be announced, or a 40-second pause reads as a hang: {:?}",
        run.stderr
    );
    assert!(
        !run.stdout.contains("rate-limited"),
        "diagnostics go to stderr; a caller pipes stdout to jq: {:?}",
        run.stdout
    );
}

#[test]
fn giving_up_after_the_attempts_is_code_five_and_still_announces_each_wait() {
    let (port, served) = scripted_server(&[(429, r#"{"error":{"message":"slow down"}}"#)], "0");

    let run = rec(port, &["--fandom", "naruto"]);

    assert_eq!(
        run.code, 5,
        "code 5 means 'come back later', not 'fix your command'"
    );
    assert_eq!(
        served.load(Ordering::SeqCst),
        3,
        "three attempts, then give up (crates/cli/src/http.rs MAX_ATTEMPTS)"
    );
    let waits = run.stderr.matches("rate-limited: waiting").count();
    assert_eq!(
        waits, 2,
        "two waits happen before the third attempt: {:?}",
        run.stderr
    );
    assert!(
        run.stderr.contains("attempts were spent"),
        "the give-up message names how many attempts were spent: {:?}",
        run.stderr
    );
}

#[test]
fn the_retry_does_not_turn_a_401_into_a_wait() {
    // Only 429 is retried. A 401 must fail immediately with code 4 and one request, or a bad token
    // would cost three round trips and a pause before saying the same thing.
    let (port, served) =
        scripted_server(&[(401, r#"{"error":{"message":"session required"}}"#)], "0");
    let run = rec(port, &["--fandom", "naruto"]);
    assert_eq!(run.code, 4);
    assert_eq!(served.load(Ordering::SeqCst), 1);
    assert!(!run.stderr.contains("rate-limited"));
}
