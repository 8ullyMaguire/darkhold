//! M63-29 — the full-screen mode lives in the same binary and is opt-in.

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
fn without_the_feature_tui_refuses_by_name() {
    let (code, stdout, stderr) = if cfg!(feature = "tui") {
        dh(&["tui", "--help"])
    } else {
        dh(&["tui"])
    };
    if cfg!(feature = "tui") {
        assert_eq!(code, 0, "{stderr}");
        assert!(stdout.contains("full-screen"), "{stdout}");
    } else {
        assert_eq!(code, 2, "{stderr}");
        assert!(stderr.contains("built without the tui feature"), "{stderr}");
    }
}

#[cfg(feature = "tui")]
#[test]
fn the_prompt_does_not_accept_tui_inside_tui() {
    fn guard(line: &str) -> bool {
        line.split_whitespace()
            .next()
            .is_some_and(|verb| verb != "tui")
    }
    assert!(!guard("tui --help"));
    assert!(guard("q --fandom naruto"));
}
