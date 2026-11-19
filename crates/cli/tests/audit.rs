//! M63-25 — `dh audit` resolves a relative window on the client; the server only accepts RFC 3339.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::sync::mpsc;

#[test]
fn audit_resolves_7d_to_rfc3339_and_prints_what_the_account_did() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut reader = BufReader::new(stream.try_clone().expect("clone"));
        let mut request = String::new();
        let _ = reader.read_line(&mut request);
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 2 {
            line.clear();
        }
        tx.send(request).expect("send request");
        let body = r#"{"items":[{"id":"a1","actor":"acct","action":"library.add","subject_type":"work","subject_id":"w1","document":"{}","created_at":"2026-11-18T00:00:00Z"}],"since":"2026-11-11T00:00:00Z","limit":5}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = stream.write_all(response.as_bytes());
    });

    let output = Command::new(env!("CARGO_BIN_EXE_dh"))
        .arg(format!("--base-url=http://127.0.0.1:{port}"))
        .arg("--token=test-token")
        .args(["audit", "--since", "7d", "--limit", "5"])
        .stdin(Stdio::null())
        .output()
        .expect("run dh");
    let text = String::from_utf8_lossy(&output.stdout);
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr: {err}");
    assert!(text.contains("library.add"), "text: {text}");
    assert!(
        text.to_ascii_lowercase().contains("what your account did"),
        "text: {text}"
    );

    let request = rx.recv().expect("request");
    assert!(
        request.contains("/api/v1/me/audit-log?"),
        "request: {request}"
    );
    let since = request
        .split("since=")
        .nth(1)
        .and_then(|part| part.split('&').next())
        .expect("since query")
        .replace("%3A", ":");
    let parsed =
        time::OffsetDateTime::parse(&since, &time::format_description::well_known::Rfc3339)
            .expect("RFC 3339 since");
    let age = time::OffsetDateTime::now_utc() - parsed;
    assert!(
        (age.whole_seconds() - 7 * 86_400).abs() < 120,
        "age: {age:?}"
    );
    assert!(request.contains("limit=5"), "request: {request}");
}
