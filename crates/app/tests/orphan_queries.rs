//! M70-13 — a database function that no reader surface can reach is either wired or deleted.
//!
//! The row was opened by a real finding: `content::list_public_works_by_freshness` was called by nothing in
//! `crates/app` — only by two tests — so the ordering guarantees it asserted were proven about a function no reader
//! could reach. It is wired now (the landing page reads it), and this test is what keeps the next one from happening.
//!
//! # What it measures
//!
//! Every `pub async fn` in `crates/db/src` must be referenced from a non-test source outside `crates/db`. A function
//! used by another db module counts, because it is part of an entry point that is reachable. A function used only by a
//! test does not: that is exactly the shape the row describes — a function that exists to satisfy its own test.
//!
//! # The ratchet
//!
//! There are hundreds of these in a crate this size, and deleting them all in one commit would be a refactor nobody
//! could review. So the test reads `docs/plans/orphan-queries.txt`: every orphan that exists today is named there with
//! a reason, and the test fails when a **new** orphan appears. The list can only shrink; the way to shrink it is to
//! wire the function or delete it, and the way to lie about it is to add a line — which the file's own header forbids
//! and a reviewer can see in the diff.
//!
//! Delete a line when its function is wired or deleted. Never add one.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/app -> crates -> workspace")
        .to_path_buf()
}

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rs_files(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
}

/// Whether the source **calls** `name`, as far as text can tell.
///
/// Three conditions, each of which closed a false green measured on 2026-11-08:
///
/// 1. the character before is not an identifier byte, so `foo` does not match `foobar`;
/// 2. the character after is `(`, so a name mentioned in prose or in a doc comment does not count — the first version of this
///    function accepted any whole identifier, and `assertions_for_work` passed on the line "inspect `assertions_for_work` before
///    anything becomes a fact" in another module's **doc comment**;
/// 3. `name(` alone is a call *or a declaration*, which is the residual this scan cannot close: `cta.rs` declares its own
///    `retract`, so the database's `retract` was reported reachable by a route handler that shares nothing with it but a word.
///    Closing that needs module paths and the caller's imports; until then the ratchet over-reports reachability, which is the
///    direction that hides work — and it is why the triage file exists rather than the scan alone.
fn mentions(source: &str, name: &str) -> bool {
    let bytes = source.as_bytes();
    let needle = name.as_bytes();
    let mut from = 0;
    while let Some(offset) = source[from..].find(name) {
        let start = from + offset;
        let end = start + needle.len();
        let before_ok = start == 0 || !is_ident_byte(bytes[start - 1]);
        let after_ok = end < bytes.len() && bytes[end] == b'(';
        if before_ok && after_ok {
            return true;
        }
        from = end;
    }
    false
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// The names declared as `pub async fn NAME` in `crates/db/src`, with the file that declares them.
fn declared(db_dir: &Path) -> Vec<(String, String)> {
    let mut files = Vec::new();
    rs_files(db_dir, &mut files);
    let mut out = Vec::new();
    for file in files {
        let Ok(source) = std::fs::read_to_string(&file) else {
            continue;
        };
        for line in source.lines() {
            let trimmed = line.trim_start();
            let Some(rest) = trimmed.strip_prefix("pub async fn ") else {
                continue;
            };
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if name.is_empty() {
                continue;
            }
            let relative = file
                .strip_prefix(workspace_root())
                .unwrap_or(&file)
                .display()
                .to_string();
            out.push((name, relative));
        }
    }
    out
}

/// Every source line outside `crates/db` and outside any `tests/` directory: the code a reader surface is built from.
fn reachable_sources() -> String {
    let root = workspace_root();
    let mut files = Vec::new();
    let crates = root.join("crates");
    for entry in std::fs::read_dir(&crates).expect("crates/").flatten() {
        let src = entry.path().join("src");
        let dir_path = entry.path();
        let name = dir_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        // `db` is the producer, and `test-support` is a **test** crate: helpers that only tests call are not a reader
        // surface, and counting them let a store function stay public on the strength of a fixture that constructs it.
        if name == "db" || name == "test-support" {
            // `name` borrows `dir_path`, which outlives the block — the temporary `entry.path()` did not.
            continue;
        }
        if src.is_dir() {
            rs_files(&src, &mut files);
        }
    }
    files
        .iter()
        .filter_map(|file| std::fs::read_to_string(file).ok())
        .collect::<Vec<_>>()
        .join("\n")
}

fn allowlisted() -> BTreeSet<String> {
    let path = workspace_root().join("docs/plans/orphan-queries.txt");
    let Ok(text) = std::fs::read_to_string(path) else {
        return BTreeSet::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.split_whitespace().next().unwrap_or(line).to_owned())
        .collect()
}

#[test]
fn no_new_database_function_is_reachable_only_from_a_test() {
    let root = workspace_root();
    let declared = declared(&root.join("crates/db/src"));
    assert!(
        declared.len() > 500,
        "the scan found only {} functions — it is not reading the tree it thinks it is",
        declared.len()
    );

    let reachable = reachable_sources();
    let allowed = allowlisted();

    let mut new_orphans: Vec<String> = Vec::new();
    for (name, file) in &declared {
        if mentions(&reachable, name) || allowed.contains(name) {
            continue;
        }
        new_orphans.push(format!("{name} ({file})"));
    }
    new_orphans.sort();

    assert!(
        new_orphans.is_empty(),
        "{} database function(s) are called by nothing outside crates/db — wire them to a route or delete them \
         (M70-13), and do not add them to docs/plans/orphan-queries.txt:\n  {}",
        new_orphans.len(),
        new_orphans.join("\n  ")
    );
}

#[test]
fn the_allowlist_shrinks_rather_than_rots() {
    // A line whose function no longer exists is a stale line, and a file that can only grow stops being a ratchet.
    let root = workspace_root();
    let declared: BTreeSet<String> = declared(&root.join("crates/db/src"))
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    let stale: Vec<String> = allowlisted()
        .into_iter()
        .filter(|name| !declared.contains(name))
        .collect();
    assert!(
        stale.is_empty(),
        "docs/plans/orphan-queries.txt names function(s) that no longer exist: {}",
        stale.join(", ")
    );
}
