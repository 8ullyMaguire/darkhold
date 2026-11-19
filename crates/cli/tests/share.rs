//! M66-08 — a reader's own files, served directly under a per-run capability.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};

fn request(url: &str) -> (u16, String) {
    let rest = url.strip_prefix("http://").expect("http url");
    let (authority, path) = rest.split_once('/').expect("authority and path");
    let mut stream = TcpStream::connect(authority).expect("connect");
    let request = format!("GET /{path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).expect("write request");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read response");
    let status = response
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .expect("status code");
    (status, response)
}

#[test]
fn share_serves_only_files_under_the_capability_prefix() {
    let dir = std::env::temp_dir().join(format!("dh-share-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture dir");
    std::fs::write(dir.join("note.txt"), "my own bytes\n").expect("fixture file");

    let mut child = Command::new(env!("CARGO_BIN_EXE_dh"))
        .args(["share", "--dir"])
        .arg(&dir)
        .args(["--port", "0"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn dh share");
    let stdout = child.stdout.take().expect("stdout");
    let mut reader = BufReader::new(stdout);
    let mut url = String::new();
    for _ in 0..40 {
        let mut line = String::new();
        let read = reader.read_line(&mut line).expect("read line");
        if read == 0 {
            break;
        }
        if let Some(value) = line.strip_prefix("SHARE_URL=") {
            url = value.trim().to_owned();
            break;
        }
    }
    assert!(!url.is_empty(), "server never printed SHARE_URL");

    let rest = url.strip_prefix("http://").expect("http url");
    let authority = rest.split_once('/').expect("authority").0;
    let bad = format!("http://{authority}/bad/");
    let (status, body) = request(&bad);
    assert_eq!(status, 404, "wrong token body: {body}");

    let file_url = format!("{url}note.txt");
    let (status, body) = request(&file_url);
    assert_eq!(status, 200, "file body: {body}");
    assert!(body.ends_with("my own bytes\n"), "file body: {body}");

    let (status, body) = request(&url);
    assert_eq!(status, 200, "index body: {body}");
    assert!(body.contains("note.txt"), "index body: {body}");

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn share_search_finds_text_under_the_capability() {
    let dir = std::env::temp_dir().join(format!("dh-share-search-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture dir");
    std::fs::write(dir.join("note.txt"), "alpha\nneedle in a haystack\n").expect("matching file");
    std::fs::write(dir.join("other.txt"), "nothing here\n").expect("other file");

    let mut child = Command::new(env!("CARGO_BIN_EXE_dh"))
        .args(["share", "--dir"])
        .arg(&dir)
        .args(["--port", "0"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn dh share");
    let stdout = child.stdout.take().expect("stdout");
    let mut reader = BufReader::new(stdout);
    let mut url = String::new();
    for _ in 0..40 {
        let mut line = String::new();
        if reader.read_line(&mut line).expect("read line") == 0 {
            break;
        }
        if let Some(value) = line.strip_prefix("SHARE_URL=") {
            url = value.trim().to_owned();
            break;
        }
    }
    assert!(!url.is_empty(), "server never printed SHARE_URL");

    let (status, body) = request(&format!("{url}?q=needle"));
    assert_eq!(status, 200, "search body: {body}");
    let json: serde_json::Value =
        serde_json::from_str(body.split("\r\n\r\n").nth(1).unwrap_or_default()).expect("JSON body");
    assert_eq!(json["query"], "needle");
    let matches = json["matches"].as_array().expect("matches");
    assert_eq!(matches.len(), 1, "body: {json}");
    assert_eq!(matches[0]["path"], "note.txt");
    assert_eq!(matches[0]["line"], 2);
    assert!(matches[0]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("needle"));

    let _ = child.kill();
    let _ = child.wait();
}
