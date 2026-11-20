//! M66-08 — a reader's own files, served directly under a per-run capability.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Command, Output, Stdio};

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

fn queue(args: &[&str], queue_file: &std::path::Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dh"))
        .arg("share-queue")
        .arg("--queue")
        .arg(queue_file)
        .args(args)
        .output()
        .expect("run share-queue")
}

#[test]
fn share_queue_adds_and_lists_while_the_peer_is_offline() {
    let dir = std::env::temp_dir().join(format!("dh-queue-offline-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture dir");
    let queue_file = dir.join("queue.json");
    let dead = "http://127.0.0.1:1/deadtoken/chapter.txt";

    let added = queue(
        &["add", "--url", dead, "--path", "chapter.txt"],
        &queue_file,
    );
    assert!(
        added.status.success(),
        "offline add failed: {}",
        String::from_utf8_lossy(&added.stderr)
    );

    let listed = queue(&["list"], &queue_file);
    assert!(listed.status.success(), "offline list failed");
    let out = String::from_utf8_lossy(&listed.stdout);
    assert!(out.contains("chapter.txt"), "list body: {out}");
    assert!(out.contains(dead), "list body: {out}");
}

#[test]
fn share_queue_drains_reachable_items_and_keeps_failed_ones() {
    let dir = std::env::temp_dir().join(format!("dh-queue-drain-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture dir");
    std::fs::write(dir.join("note.txt"), "queued bytes\n").expect("fixture file");
    let queue_file = dir.join("queue.json");

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

    assert!(queue(
        &[
            "add",
            "--url",
            &format!("{url}note.txt"),
            "--path",
            "note.txt"
        ],
        &queue_file,
    )
    .status
    .success());
    assert!(queue(
        &[
            "add",
            "--url",
            &format!("{url}absent.txt"),
            "--path",
            "absent.txt",
        ],
        &queue_file,
    )
    .status
    .success());

    let out_dir = dir.join("received");
    std::fs::create_dir_all(&out_dir).expect("out dir");
    let drained = queue(&["drain", "--out", out_dir.to_str().unwrap()], &queue_file);
    assert!(
        drained.status.success(),
        "drain failed: {}",
        String::from_utf8_lossy(&drained.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(out_dir.join("note.txt")).expect("drained file"),
        "queued bytes\n"
    );
    assert!(!out_dir.join("absent.txt").exists(), "404 was written");

    let listed = queue(&["list"], &queue_file);
    let out = String::from_utf8_lossy(&listed.stdout);
    assert!(
        out.contains("absent.txt"),
        "failed item was discarded: {out}"
    );
    assert!(
        !out.contains("note.txt"),
        "successful item stayed queued: {out}"
    );

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn share_queue_refuses_paths_outside_the_output_root() {
    let dir = std::env::temp_dir().join(format!("dh-queue-guard-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture dir");
    let queue_file = dir.join("queue.json");

    let added = queue(
        &[
            "add",
            "--url",
            "http://127.0.0.1:1/token/escape.txt",
            "--path",
            "../escape.txt",
        ],
        &queue_file,
    );
    assert_eq!(
        added.status.code(),
        Some(2),
        "guard did not refuse: {added:?}"
    );
    assert!(!queue_file.exists(), "refused path still wrote a queue");
}

#[test]
fn share_queue_suggests_from_local_co_queued_edges() {
    let dir = std::env::temp_dir().join(format!("dh-queue-edges-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture dir");
    let queue_file = dir.join("queue.json");
    for path in ["a.txt", "b.txt", "c.txt"] {
        let added = queue(
            &[
                "add",
                "--url",
                &format!("http://127.0.0.1:1/deadtoken/{path}"),
                "--path",
                path,
            ],
            &queue_file,
        );
        assert!(added.status.success(), "add {path} failed");
    }

    let suggested = queue(&["suggest", "--for", "a.txt"], &queue_file);
    assert!(suggested.status.success(), "suggest failed");
    let json: serde_json::Value = serde_json::from_slice(&suggested.stdout).expect("suggest JSON");
    assert_eq!(json["for"], "a.txt");
    let names: Vec<&str> = json["suggestions"]
        .as_array()
        .expect("suggestions array")
        .iter()
        .filter_map(|value| value.as_str())
        .collect();
    assert_eq!(names, ["b.txt", "c.txt"], "local edges: {json}");
}
