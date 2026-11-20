use std::fs;
use std::os::unix::fs::PermissionsExt;

fn prefs(mode: u32) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "darkhold-archive-cli-prefs-{}-{}.json",
        std::process::id(),
        mode
    ));
    fs::write(
        &path,
        r#"{"version":1,"positive":[{"term":"dark mc","weight":1.0}],"negative":["slash"]}"#,
    )
    .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
    path
}

#[test]
fn private_preferences_must_be_mode_0600() {
    let path = prefs(0o644);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_dh"))
        .args([
            "archive",
            "--db-url",
            "sqlite::memory:",
            "--account",
            "01956c00-0000-4000-8000-000000000001",
            "--prefs",
        ])
        .arg(&path)
        .arg("rank")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("mode 0600"), "stderr: {stderr}");
    let _ = fs::remove_file(path);
}

#[test]
fn bodies_only_enqueue_private_taste_matches() {
    let source = include_str!("../src/archive.rs");
    assert!(
        source.contains("if scored.excluded {"),
        "excluded candidates must be filtered out"
    );
    assert!(
        source.contains("if scored.value <= 0.0 {"),
        "only positive taste matches may be selected"
    );
    assert!(
        source.contains("\"body_policy\": \"store\""),
        "selected works must request body storage"
    );
}

#[test]
fn no_global_body_ceiling_remains() {
    let archive = include_str!("../src/archive.rs");
    let prefs = include_str!("../src/archive_preferences.rs");
    assert!(
        !archive.contains("max_bodies"),
        "archive must not carry a product body ceiling"
    );
    assert!(
        !prefs.contains("max_bodies"),
        "private preferences must not carry a product body ceiling"
    );
    assert!(
        archive.contains("let cap = *limit;"),
        "the per-run --limit remains the only brake"
    );
}
