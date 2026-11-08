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

use std::collections::{BTreeMap, BTreeSet};
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

/// Every top-level function in `crates/db/src`, with the text of its body.
///
/// A body is "the lines after its declaration and before the next declaration". That is a crude parse, and it is the right
/// crude parse here: it cannot be fooled by a brace inside a string, a closure, or a `match` arm, because it never counts
/// braces at all. A function nested inside another would be mis-sliced as a sibling, and a macro that declares functions
/// would be invisible; neither exists in this crate, and both would show up as an unexpected name in the reachable set
/// rather than as a silent pass.
fn bodies(db_dir: &Path) -> BTreeMap<String, String> {
    const PREFIXES: [&str; 6] = [
        "pub async fn ",
        "pub(crate) async fn ",
        "async fn ",
        "pub fn ",
        "pub(crate) fn ",
        "fn ",
    ];
    let mut files = Vec::new();
    rs_files(db_dir, &mut files);
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    for file in files {
        let Ok(source) = std::fs::read_to_string(&file) else {
            continue;
        };
        let mut current: Option<String> = None;
        let mut buffer = String::new();
        let flush = |current: &mut Option<String>,
                     buffer: &mut String,
                     out: &mut BTreeMap<String, String>| {
            if let Some(name) = current.take() {
                out.entry(name).or_default().push_str(buffer);
            }
            buffer.clear();
        };
        for line in source.lines() {
            // Indentation is part of the question, not a detail: `connect`, `migrate`, `ping` and the whole `BlobStore`
            // surface are methods inside `impl` blocks, and a scan that only reads column 0 sees their names in
            // `declared()` and never in this map — so they can never be a root and never be reached.
            let declared = PREFIXES.iter().find_map(|prefix| {
                let rest = line.trim_start().strip_prefix(prefix)?;
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                (!name.is_empty()).then_some(name)
            });
            match declared {
                Some(name) => {
                    flush(&mut current, &mut buffer, &mut out);
                    current = Some(name);
                }
                None => {
                    if current.is_some() {
                        buffer.push_str(line);
                        buffer.push('\n');
                    }
                }
            }
        }
        flush(&mut current, &mut buffer, &mut out);
    }
    out
}

/// Which declared functions are reachable once `crates/db` is allowed to call itself.
///
/// The scan's first version asked only "does any *app* source mention this name", which makes every internal helper an
/// orphan: `velocity_signals`, `award`, `policy_version` and five `work_characters` writers were all reported dead while a
/// route reached each of them through one more call. Reachability is transitive, so the answer is a closure — roots are the
/// names an app source mentions, and the edges are db functions mentioning one another.
fn reachable_via_db(declared: &BTreeMap<String, String>, app_sources: &str) -> BTreeSet<String> {
    let mut queue: Vec<String> = declared
        .keys()
        .filter(|name| mentions(app_sources, name))
        .cloned()
        .collect();
    let mut seen: BTreeSet<String> = queue.iter().cloned().collect();
    while let Some(name) = queue.pop() {
        let Some(body) = declared.get(&name) else {
            continue;
        };
        for other in declared.keys() {
            if seen.contains(other) || other == &name {
                continue;
            }
            if mentions(body, other) {
                seen.insert(other.clone());
                queue.push(other.clone());
            }
        }
    }
    seen
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

    let app_sources = reachable_sources();
    let bodies = bodies(&root.join("crates/db/src"));
    let reachable = reachable_via_db(&bodies, &app_sources);
    let allowed = allowlisted();

    let mut new_orphans: Vec<String> = Vec::new();
    for (name, file) in &declared {
        if reachable.contains(name) || allowed.contains(name) {
            continue;
        }
        new_orphans.push(format!("{name} ({file})"));
    }
    new_orphans.sort();

    assert!(
        new_orphans.is_empty(),
        "{} database function(s) are reachable from no app source, directly or through another db function — wire them to a \
         route or delete them (M70-13), and do not add them to docs/plans/orphan-queries.txt:\n  {}",
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
