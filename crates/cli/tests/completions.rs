//! `M63-17` — completions and the man page come out of the parser, and these tests prove it.
//!
//! A hand-written completion list is wrong the first time a flag is added and nobody notices, because
//! nobody re-reads a completion file in review. The invariant that makes generation worth the two
//! dependencies is **agreement with the parser**, so the test takes the subcommand list from
//! `dh --help` — the parser's own description of itself — and requires every name to appear in every
//! generated artefact. A hand-written list that drifts fails here.
//!
//! Two other things are pinned because they are how people actually use this:
//!
//! - **the artefacts go to stdout**, so `> file` and `| head` work;
//! - **a closed pipe is not a panic**. `clap_complete` unwraps its own write, so the first version
//!   died with `failed to write completion file: BrokenPipe` when piped into `head` — which is the
//!   first thing anyone does with a completion script.

use std::process::{Command, Stdio};

fn run(args: &[&str]) -> (i32, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_dh"))
        .args(args)
        .env_remove("DARKHOLD_TOKEN")
        .env_remove("DARKHOLD_COOKIE")
        .output()
        .expect("run dh");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// The subcommands as the parser itself lists them, under `Commands:` in `--help`.
fn subcommands_from_help() -> Vec<String> {
    let (code, help, stderr) = run(&["--help"]);
    assert_eq!(code, 0, "dh --help: {stderr}");
    let mut names = Vec::new();
    let mut inside = false;
    for line in help.lines() {
        if line.starts_with("Commands:") {
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        if line.trim().is_empty() {
            break;
        }
        if let Some(name) = line.split_whitespace().next() {
            if name.chars().all(|c| c.is_ascii_lowercase() || c == '-') {
                names.push(name.to_owned());
            }
        }
    }
    assert!(
        names.len() >= 3,
        "the help parser found {names:?} — if the help layout changed, fix the parser rather than \
         letting this gate check nothing"
    );
    names
}

#[test]
fn every_shell_gets_a_script_that_names_every_subcommand() {
    let names = subcommands_from_help();
    for shell in ["bash", "zsh", "fish"] {
        let (code, script, stderr) = run(&["completions", shell]);
        assert_eq!(code, 0, "{shell}: {stderr}");
        assert!(!script.is_empty(), "{shell} produced nothing on stdout");
        for name in &names {
            assert!(
                script.contains(name),
                "the {shell} completion script never mentions `{name}`, so it was not generated \
                 from this parser"
            );
        }
    }
}

#[test]
fn the_man_page_is_generated_and_names_the_subcommands() {
    let names = subcommands_from_help();
    let (code, page, stderr) = run(&["man"]);
    assert_eq!(code, 0, "{stderr}");
    assert!(
        page.contains(".TH dh"),
        "a roff man page starts with .TH: {:?}",
        page.lines().next()
    );
    for name in &names {
        assert!(page.contains(name), "the man page never mentions `{name}`");
    }
}

#[test]
fn a_closed_pipe_is_not_a_panic() {
    // `| head` is how anyone looks at a completion script before installing it. The process must exit
    // 0 with nothing on stderr; a panic here would be reported as a broken tool by the shell.
    let output = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "{} completions zsh | head -2",
            env!("CARGO_BIN_EXE_dh")
        ))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("run through sh");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked"),
        "a closed pipe panicked the process: {stderr}"
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "the pipeline's status is head's, and head succeeded"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.lines().count(),
        2,
        "head -2 got two lines: {stdout:?}"
    );
}

#[test]
fn an_unknown_shell_is_a_usage_error_before_anything_is_generated() {
    let (code, stdout, _) = run(&["completions", "tcsh"]);
    assert_eq!(code, 2, "clap rejects the value; nothing was generated");
    assert!(stdout.is_empty(), "nothing should reach stdout: {stdout:?}");
}
