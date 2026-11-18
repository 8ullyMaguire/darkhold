//! M63-26 — the personal report is private by default and shareable only when asked.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};

use base64::Engine;
use darkhold_domain::passport::{Contributions, Passport, VERSION};

fn dh(args: &[&str]) -> (i32, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_dh"))
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .expect("run dh");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn stats_help_names_the_period_and_share_flags() {
    let (code, stdout, stderr) = dh(&["stats", "--help"]);
    assert_eq!(code, 0, "{stderr}");
    for word in ["--by", "week", "year", "--year", "--share"] {
        assert!(stdout.contains(word), "help omitted {word}: {stdout}");
    }
}

#[test]
fn stats_without_a_credential_is_refused_before_any_report() {
    let (code, _stdout, stderr) = dh(&["stats", "--by", "week"]);
    assert_eq!(code, 4, "{stderr}");
    assert!(stderr.to_ascii_lowercase().contains("token"), "{stderr}");
}

fn fake_instance() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let passport = Passport {
        version: VERSION,
        origin: "https://origin.test".to_owned(),
        subject: "secret-handle".to_owned(),
        issued_at: "2026-11-18T00:00:00Z".to_owned(),
        taste: std::collections::BTreeMap::new(),
        favourite_sources: Vec::new(),
        contributions: Contributions {
            works: 3,
            revisions: 27,
            comments: 11,
        },
        affinity_bp: None,
    };
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&passport).expect("passport json"));
    let signed = format!("header.{payload}.signature");
    std::thread::spawn(move || {
        for _ in 0..2 {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let mut request = String::new();
            let _ = reader.read_line(&mut request);
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) > 2 {
                line.clear();
            }
            let body = if request.contains("/me/streak") {
                r#"{"current_streak":12,"longest_streak":40,"last_login_at":"2026-11-18","streak_freezes_used":1}"#.to_owned()
            } else {
                serde_json::json!({"passport": signed, "note": "signed"}).to_string()
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    port
}

#[test]
fn the_report_uses_the_servers_private_surfaces_and_the_share_flag_hides_identity() {
    let port = fake_instance();
    let base = format!("--base-url=http://127.0.0.1:{port}");
    let output = Command::new(env!("CARGO_BIN_EXE_dh"))
        .arg(&base)
        .arg("--token=test-token")
        .args(["stats", "--by", "week"])
        .stdin(Stdio::null())
        .output()
        .expect("run dh");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("secret-handle"), "{text}");
    assert!(text.contains("revisions made        27"), "{text}");

    let port = fake_instance();
    let base = format!("--base-url=http://127.0.0.1:{port}");
    let output = Command::new(env!("CARGO_BIN_EXE_dh"))
        .arg(&base)
        .arg("--token=test-token")
        .args(["stats", "--by", "week", "--share"])
        .stdin(Stdio::null())
        .output()
        .expect("run dh");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(!text.contains("secret-handle"), "{text}");
    assert!(text.contains("revisions made        27"), "{text}");
}
