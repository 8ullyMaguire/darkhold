//! M63-29: a full-screen, line-driven mode inside the same `dh` binary.
//!
//! This module is compiled only with `--features tui`. It does not own a parser: every prompt line is handed back to
//! `dh` itself with `--json`, so the window grammar, saved queries and exit codes stay in exactly one place. The only
//! thing here is the alternate-screen output layer.

use std::io::{self, Write};
use std::process::Command as ProcessCommand;

use crate::rec::{environment, Failure};

/// Verbs the prompt will dispatch. `tui` is refused so the mode cannot nest itself.
const VERBS: [&str; 6] = ["rec", "q", "library", "supply", "module", "config"];

#[must_use]
pub fn allowed(line: &str) -> bool {
    line.split_whitespace()
        .next()
        .is_some_and(|verb| VERBS.contains(&verb))
}

/// The whole screen. Kept pure so the renderer has a test and the escape sequences stay in one line.
#[must_use]
pub fn render(output: &str, prompt: &str) -> String {
    let mut screen = String::from("\x1b[2J\x1b[H dh tui — q to quit\n");
    screen.push_str("────────────────────────────────────────\n");
    for line in output.lines().take(18) {
        screen.push_str(line);
        screen.push('\n');
    }
    screen.push_str("────────────────────────────────────────\n");
    screen.push_str(prompt);
    screen.push_str("> ");
    screen
}

/// Enter the alternate screen, render each command's JSON output, and restore the terminal on `q`.
pub fn run(cli: &crate::Cli) -> Result<(), Failure> {
    let exe = std::env::current_exe().map_err(|error| environment(format!("own path: {error}")))?;
    let mut output =
        String::from("Type a verb from rec, q, library, supply, module or config. q quits.");
    let mut stdout = io::stdout();
    print!("\x1b[?1049h");
    let _ = stdout.flush();

    let result = loop {
        print!("{}", render(&output, ""));
        if stdout.flush().is_err() {
            break Err(environment("cannot write to the terminal"));
        }
        let mut line = String::new();
        match io::stdin().read_line(&mut line) {
            Ok(0) => break Ok(()),
            Ok(_) => {}
            Err(error) => break Err(environment(format!("cannot read the prompt: {error}"))),
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line == "q" || line == "quit" {
            break Ok(());
        }
        if !allowed(line) {
            output = format!("refused: `{line}` is not a prompt verb; no nested tui");
            continue;
        }
        output = dispatch(&exe, cli, line);
    };

    print!("\x1b[?1049l");
    let _ = stdout.flush();
    result
}

fn dispatch(exe: &std::path::Path, cli: &crate::Cli, line: &str) -> String {
    let mut command = ProcessCommand::new(exe);
    if !cli.base_url.is_empty() {
        command.args(["--base-url", cli.base_url.trim_end_matches('/')]);
    }
    if let Some(profile) = &cli.profile {
        command.args(["--profile", profile]);
    }
    command.arg("--json");
    command.args(line.split_whitespace());
    if let Some(token) = &cli.token {
        command.env("DARKHOLD_TOKEN", token);
    }
    if let Some(cookie) = &cli.cookie {
        command.env("DARKHOLD_COOKIE", cookie);
    }
    match command.output() {
        Ok(result) => {
            let mut text = String::from_utf8_lossy(&result.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&result.stderr);
            if !stderr.is_empty() {
                text.push('\n');
                text.push_str(&stderr);
            }
            if text.trim().is_empty() {
                format!("(no output; exit {})", result.status.code().unwrap_or(-1))
            } else {
                text
            }
        }
        Err(error) => format!("could not run dh: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_screen_keeps_the_header_and_the_output() {
        let screen = render("line one\nline two", "q");
        assert!(screen.contains("q to quit"));
        assert!(screen.contains("line one"));
        assert!(screen.contains("line two"));
        assert!(screen.ends_with("q> "));
    }

    #[test]
    fn tui_cannot_nest_and_unknown_verbs_are_refused() {
        assert!(!allowed("tui --help"));
        assert!(!allowed("doctor --json"));
        assert!(allowed("rec --fandom naruto"));
    }
}
