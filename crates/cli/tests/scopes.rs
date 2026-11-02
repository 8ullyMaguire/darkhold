//! `M63-19` — a refusal says which scope is missing, or says plainly that no scope will help.
//!
//! A `403` used to reach the user as the server's sentence and nothing else, and each verb added its
//! own guess about what to do: `dh rec` said "this engine reads your own history", which is true of
//! `rec` and false of `dh supply complete`. None of them could name the scope, because the server
//! carried it only inside prose — so the one thing a caller could act on was the one thing missing.
//!
//! Two refusals, and they are not the same refusal:
//!
//! - **The token is too small.** `required_scope` is present; the fix is to mint a wider token, and
//!   the message names the scope to add.
//! - **The account's role was refused.** There is no scope; suggesting one would send the caller to
//!   mint a token that still would not open the door. This is the case a status code alone cannot
//!   distinguish, which is why the test pins both.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};

/// A server that answers every request with `status` and `body`.
fn refusing_server(status: u16, body: &'static str) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
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
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let mut stream = stream;
            let _ = stream.write_all(response.as_bytes());
        }
    });
    port
}

fn run(port: u16, args: &[&str]) -> (i32, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_dh"))
        .arg("--base-url")
        .arg(format!("http://127.0.0.1:{port}"))
        .args(args)
        .env("DARKHOLD_TOKEN", "a-token-with-one-scope")
        .stdin(Stdio::null())
        .output()
        .expect("run dh");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn a_scope_refusal_names_the_scope_and_how_to_get_one() {
    let port = refusing_server(
        403,
        r#"{"error":{"code":"ACCESS_DENIED","message":"this token lacks the content.write scope","required_scope":"content.write","request_id":"r1"}}"#,
    );
    let (code, _stdout, stderr) = run(port, &["rec", "--fandom", "naruto"]);

    assert_eq!(
        code, 4,
        "a scope refusal is an authorisation failure: {stderr}"
    );
    assert!(
        stderr.contains("content.write"),
        "the message names the scope to add: {stderr}"
    );
    assert!(
        stderr.contains("/settings/tokens"),
        "and where to mint one: {stderr}"
    );
    assert!(
        stderr.contains("DARKHOLD_COOKIE"),
        "and the alternative that needs no scope: {stderr}"
    );
}

#[test]
fn a_role_refusal_does_not_invent_a_scope() {
    let port = refusing_server(
        403,
        r#"{"error":{"code":"ACCESS_DENIED","message":"Shared attribution changes require curator authority.","request_id":"r2"}}"#,
    );
    let (code, _stdout, stderr) = run(port, &["rec", "--fandom", "naruto"]);

    assert_eq!(code, 4, "{stderr}");
    assert!(
        stderr.contains("role"),
        "the message says the role was refused: {stderr}"
    );
    assert!(
        stderr.contains("not a scope"),
        "and says so explicitly, because the two are indistinguishable from the status: {stderr}"
    );
    assert!(
        !stderr.contains("settings/tokens"),
        "nothing sends the caller to mint a token that would not work: {stderr}"
    );
}

#[test]
fn a_missing_session_is_not_reported_as_a_scope_problem() {
    let port = refusing_server(
        401,
        r#"{"error":{"code":"AUTH_REQUIRED","message":"sign in to continue","request_id":"r3"}}"#,
    );
    let (code, _stdout, stderr) = run(port, &["rec", "--fandom", "naruto"]);

    assert_eq!(code, 4, "{stderr}");
    assert!(
        stderr.contains("DARKHOLD_TOKEN"),
        "a 401 names the credential that is missing: {stderr}"
    );
    assert!(
        !stderr.contains("lacks the"),
        "and does not claim a token is too small when there is no token: {stderr}"
    );
}

#[test]
fn the_scope_refusal_is_the_same_sentence_for_every_verb() {
    // `dh supply complete` and `dh rec` used to describe a `403` differently, because each wrote its
    // own sentence. One refusal path means one story.
    let port = refusing_server(
        403,
        r#"{"error":{"code":"ACCESS_DENIED","message":"this token lacks the admin.write scope","required_scope":"admin.write","request_id":"r4"}}"#,
    );
    let (rec_code, _, rec_err) = run(port, &["rec", "--fandom", "naruto"]);
    let (supply_code, _, supply_err) = run(
        port,
        &[
            "supply",
            "complete",
            "00000000-0000-0000-0000-000000000001",
            "--yes",
        ],
    );

    assert_eq!(rec_code, 4, "{rec_err}");
    assert_eq!(supply_code, 4, "{supply_err}");
    assert!(rec_err.contains("admin.write") && supply_err.contains("admin.write"));
    // Same hint, same wording — the only difference allowed is the server's own message.
    let tail = |s: &str| s.lines().last().unwrap_or_default().to_owned();
    assert_eq!(
        tail(&rec_err),
        tail(&supply_err),
        "the advice must not depend on which verb asked"
    );
}
