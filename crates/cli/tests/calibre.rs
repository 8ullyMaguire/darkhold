//! M63-30 — Calibre is a file handshake, not a dependency or a service.

use std::path::PathBuf;
use std::process::{Command, Stdio};

fn fixture() -> PathBuf {
    let path = std::env::temp_dir().join(format!("dh-calibre-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let sql = r#"
CREATE TABLE books (id INTEGER PRIMARY KEY, title TEXT NOT NULL, rating INTEGER NOT NULL DEFAULT 0);
CREATE TABLE authors (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE books_authors_link (book INTEGER NOT NULL, author INTEGER NOT NULL);
INSERT INTO books VALUES (1, 'A Study in Green', 8), (2, 'Quiet Roads', 4), (3, 'Unrated Notes', 0);
INSERT INTO authors VALUES (7, 'A. Reader'), (8, 'B. Writer');
INSERT INTO books_authors_link VALUES (1, 7), (2, 7), (2, 8);
"#;
    let status = Command::new("sqlite3")
        .arg(&path)
        .arg(sql)
        .status()
        .expect("sqlite3");
    assert!(status.success());
    path
}

fn dh(args: &[&str]) -> (i32, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_dh"))
        .args(args)
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
fn calibre_reads_the_grammar_without_touching_the_source_database() {
    let path = fixture();
    let before = std::fs::read(&path).expect("fixture bytes");
    let db = path.to_string_lossy().into_owned();

    let (code, stdout, stderr) = dh(&["lib", "--calibre", &db]);
    assert_eq!(code, 0, "{stderr}");
    assert!(
        stdout.contains("1  8  A Study in Green  A. Reader"),
        "{stdout}"
    );
    assert!(
        stdout.contains("2  4  Quiet Roads  A. Reader & B. Writer"),
        "{stdout}"
    );
    assert!(!stdout.contains("Unrated Notes"), "{stdout}");
    assert_eq!(
        std::fs::read(&path).expect("fixture bytes"),
        before,
        "Calibre was modified"
    );

    let (code, stdout, stderr) = dh(&["lib", "--calibre", &db, "--json"]);
    assert_eq!(code, 0, "{stderr}");
    for word in [
        "\"id\": 1",
        "\"rating\": 8",
        "\"title\": \"A Study in Green\"",
        "\"author\": \"A. Reader\"",
    ] {
        assert!(stdout.contains(word), "json omitted {word}: {stdout}");
    }
    let _ = std::fs::remove_file(path);
}
