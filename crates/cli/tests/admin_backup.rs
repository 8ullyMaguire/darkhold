//! M63-23: operator dump streams locally; health is the existing doctor report.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

fn dh() -> String {
    env!("CARGO_BIN_EXE_dh").to_string()
}

#[test]
fn admin_backup_dash_streams_a_local_dump() {
    let dir = std::env::temp_dir().join(format!("dh-backup-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch dir");
    let fake = dir.join("pg_dump");
    fs::write(&fake, "#!/bin/sh\nprintf 'FAKE_DUMP\\n'\n").expect("fake pg_dump");
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).expect("executable");

    let output = Command::new(dh())
        .args(["admin", "backup", "--to", "-"])
        .env(
            "PATH",
            format!(
                "{}:{}",
                dir.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .env("PGDATABASE", "darkhold_test")
        .env_remove("DARKHOLD_TEST_PG_URL")
        .output()
        .expect("dh admin backup");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "FAKE_DUMP\n");
}

#[test]
fn admin_backup_refuses_a_file_path() {
    let output = Command::new(dh())
        .args(["admin", "backup", "--to", "/tmp/leak.dump"])
        .output()
        .expect("dh admin backup");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("stdout"), "{stderr}");
}
