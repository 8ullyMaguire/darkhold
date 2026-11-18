//! M63-12 — `dh watch` is a dry-run plan by default; a real daemon needs --yes.

use std::process::Command;

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
fn watch_help_names_the_saved_query_exec_and_notification_sinks() {
    let (code, stdout, stderr) = dh(&["watch", "--help"]);
    assert_eq!(code, 0, "{stderr}");
    for word in ["saved", "--exec", "--notify", "ntfy", "desktop", "webhook"] {
        assert!(stdout.contains(word), "help omitted {word}: {stdout}");
    }
}

#[test]
fn a_watch_plan_does_not_poll_or_execute_without_yes() {
    let (code, stdout, stderr) = dh(&[
        "watch",
        "weekly",
        "--exec",
        "touch /tmp/dh-watch-must-not-run",
        "--notify",
        "webhook",
        "--every",
        "6h",
        "--debounce",
        "10m",
        "--dry-run",
    ]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("would watch"), "{stdout}");
    assert!(stdout.contains("--yes"), "{stdout}");
    assert!(!std::path::Path::new("/tmp/dh-watch-must-not-run").exists());
}
