//! `M63-16` — the exit codes are a contract, and this is the test that holds it.
//!
//! A wrapper script decides what to do from this number alone, so the test drives the **real
//! binary** (`CARGO_BIN_EXE_dh`) against a canned server that answers one status and nothing else.
//! Unit-testing `for_status` would prove the table; running `dh` proves the table is what the process
//! actually exits with, which is the part a shell sees.
//!
//! The mapping's history is the reason the codes are split: `401`, `403` and `404` were all reported
//! as `2` (usage), so a script was told "your command is wrong" when the truth was "no session" or
//! "that work is gone" — the two cases it can act on were indistinguishable from the one it cannot.
//!
//! Codes: `0` ok, `1` environment, `2` usage, `3` not found, `4` auth, `5` rate-limited.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::Command;

/// Serve exactly one request with `status` and `body`, then stop. Returns the port.
fn canned_server(status: u16, body: &'static str) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            // Drain the request line and headers; the body is irrelevant to every case here.
            let mut line = String::new();
            while reader.read_line(&mut line).map(|n| n > 0).unwrap_or(false) {
                if line == "\r\n" {
                    break;
                }
                line.clear();
            }
            let reason = match status {
                200 => "OK",
                401 => "Unauthorized",
                404 => "Not Found",
                422 => "Unprocessable Entity",
                429 => "Too Many Requests",
                500 => "Internal Server Error",
                _ => "Status",
            };
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\nRetry-After: 0\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let mut stream = stream;
            let _ = stream.write_all(response.as_bytes());
        }
    });
    port
}

/// Like [`canned_server`] but answers `times` requests, for the retrying path (`M63-20`).
fn canned_server_repeating(status: u16, body: &'static str, times: usize) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming().take(times) {
            let Ok(stream) = stream else { break };
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let mut line = String::new();
            while reader.read_line(&mut line).map(|n| n > 0).unwrap_or(false) {
                if line == "\r\n" {
                    break;
                }
                line.clear();
            }
            let response = format!(
                "HTTP/1.1 {status} Status\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\nRetry-After: 0\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let mut stream = stream;
            let _ = stream.write_all(response.as_bytes());
        }
    });
    port
}

/// Run `dh` and return its exit code.
fn dh(args: &[&str]) -> i32 {
    let output = Command::new(env!("CARGO_BIN_EXE_dh"))
        .args(args)
        .env_remove("DARKHOLD_TOKEN")
        .env_remove("DARKHOLD_COOKIE")
        .output()
        .expect("run dh");
    output
        .status
        .code()
        .unwrap_or_else(|| panic!("dh was killed by a signal: {:?}", output))
}

fn dh_against(port: u16, tail: &[&str]) -> i32 {
    let url = format!("http://127.0.0.1:{port}");
    let mut args = vec!["--base-url".to_owned(), url];
    args.extend(tail.iter().map(|s| (*s).to_owned()));
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    dh(&refs)
}

#[test]
fn a_not_found_is_three_not_a_usage_error() {
    let port = canned_server(404, r#"{"error":{"message":"no work called nosuch"}}"#);
    assert_eq!(
        dh_against(port, &["rec", "--fandom", "nosuchfandom"]),
        3,
        "a script must be able to tell 'deleted/stale' (3) from 'you typed it wrong' (2)"
    );
}

#[test]
fn a_local_refusal_that_needs_a_session_is_four() {
    // `--engine` reads the caller's own history, so `dh` refuses before sending anything. That
    // refusal must carry the same code a `401` would, or a wrapper learns a different number
    // depending on whether the check happened here or on the server.
    let port = canned_server(500, r#"{"error":{"message":"should not be reached"}}"#);
    assert_eq!(
        dh_against(port, &["rec", "--fandom", "naruto", "--engine", "rrf"]),
        4,
        "a missing session is 4 whether this binary or the server noticed it"
    );
}

#[test]
fn a_server_refusal_of_a_session_is_four() {
    // The HTTP path, which the local refusal above deliberately short-circuits: `module show` sends
    // its request with no session and lets the server answer, so this is the test that proves
    // `for_status(401)` is what the process exits with — not only what the unit test believes.
    let port = canned_server(401, r#"{"error":{"message":"session required"}}"#);
    assert_eq!(
        dh_against(port, &["module", "show", "concierge"]),
        4,
        "the server's 401 must reach the shell as 4"
    );
}

#[test]
fn a_rate_limit_is_five() {
    // `M63-20` made `429` retryable, so this test needs a server that answers three times and a
    // `Retry-After: 0` so it does not spend a real minute asleep. The assertion is unchanged: after
    // the attempts are spent, the code is 5.
    let port = canned_server_repeating(429, r#"{"error":{"message":"slow down"}}"#, 3);
    assert_eq!(
        dh_against(port, &["rec", "--fandom", "naruto"]),
        5,
        "429 is the one status whose correct response is a delayed retry"
    );
}

#[test]
fn a_bad_value_is_usage_two() {
    let port = canned_server(422, r#"{"error":{"message":"fandom: unknown"}}"#);
    assert_eq!(dh_against(port, &["rec", "--fandom", "naruto"]), 2);
}

#[test]
fn a_server_failure_is_one() {
    let port = canned_server(500, r#"{"error":{"message":"boom"}}"#);
    assert_eq!(dh_against(port, &["rec", "--fandom", "naruto"]), 1);
}

#[test]
fn nothing_listening_is_one() {
    // Port 1 on loopback refuses immediately; no server is started.
    let url = "http://127.0.0.1:1";
    assert_eq!(
        dh(&["--base-url", url, "rec", "--fandom", "naruto"]),
        1,
        "a connection failure is environmental, so it is retryable rather than a usage error"
    );
}

#[test]
fn a_flag_that_does_not_parse_is_usage_two_before_any_request() {
    let port = canned_server(500, r#"{"error":{"message":"should not be reached"}}"#);
    assert_eq!(
        dh_against(port, &["rec", "--fandom", "naruto", "--nonsense"]),
        2,
        "clap exits 2 for a parse failure, which is why the usage code is 2 and not 1"
    );
}

#[test]
fn a_local_verb_succeeds_with_zero() {
    // `--list-metrics` prints the shared vocabulary and never touches the network, so this is the
    // control: code 0 is reachable and is not the same number as any failure.
    let port = canned_server(500, r#"{"error":{"message":"should not be reached"}}"#);
    assert_eq!(dh_against(port, &["rec", "--list-metrics"]), 0);
}
