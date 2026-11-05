//! M63-39 — the shipped trope queries parse.
//!
//! `.darkhold/queries/**/*.q` is a versioned library: an operator runs
//! `dh q --from-file tropes/dark-progression`, and "what does this instance consider dark
//! progression" is a file in review. A file that no longer parses would fail at the operator's
//! evening rather than in CI, so this test reads every shipped query and runs it through the same
//! parser the server uses, with the same comment rule the CLI applies.

use std::path::{Path, PathBuf};

/// The library root, relative to this crate.
fn library() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.darkhold/queries")
}

/// Cut a `#` comment: only at the start of the line or after whitespace, because a tag can be
/// spelled `#angst`. This must agree with `drop_comment` in `crates/cli/src/q.rs`; if the two drift,
/// this test proves the wrong thing, which is why the rule is asserted below as well.
fn drop_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    for index in 0..bytes.len() {
        if bytes[index] == b'#' && (index == 0 || bytes[index - 1].is_ascii_whitespace()) {
            return &line[..index];
        }
    }
    line
}

fn expression(raw: &str) -> String {
    let mut expression = String::new();
    for line in raw.lines() {
        let text = drop_comment(line).trim();
        if text.is_empty() {
            continue;
        }
        if !expression.is_empty() {
            expression.push(' ');
        }
        expression.push_str(text);
    }
    expression
}

/// Every `.q` file under the library, so a new one is covered without editing this test.
fn shipped() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "q") {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(&library(), &mut out);
    out.sort();
    out
}

#[test]
fn the_comment_rule_agrees_with_the_cli() {
    assert_eq!(drop_comment("# whole line"), "");
    assert_eq!(drop_comment("  # indented comment"), "  ");
    assert_eq!(
        drop_comment("tag:a # note"),
        "tag:a ",
        "a comment after whitespace is cut"
    );
    assert_eq!(
        drop_comment("tag:#angst"),
        "tag:#angst",
        "a # inside a value is not a comment"
    );
}

#[test]
fn every_shipped_trope_query_parses() {
    let files = shipped();
    assert!(
        !files.is_empty(),
        "no .q files under {} — the library is the deliverable, not a directory",
        library().display()
    );

    let mut failures = Vec::new();
    for path in &files {
        let raw = std::fs::read_to_string(path).expect("readable");
        let joined = expression(&raw);
        assert!(
            !joined.trim().is_empty(),
            "{}: no expression — every line is blank or a comment",
            path.display()
        );
        if let Err(error) = darkhold_domain::query::parse_query(&joined) {
            failures.push(format!(
                "{}: {} at offset {} — {}",
                path.display(),
                error.message,
                error.offset,
                joined
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "shipped queries that do not parse: {failures:#?}"
    );
    println!("{} shipped trope queries parse", files.len());
}
