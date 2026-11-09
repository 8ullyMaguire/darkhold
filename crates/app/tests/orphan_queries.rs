//! M70-13 — a database function that no reader surface can reach is either wired or deleted.
//!
//! The row was opened by a real finding: `content::list_public_works_by_freshness` was called by nothing in `crates/app` — only by
//! two tests — so the ordering guarantees it asserted were proven about a function no reader could reach. It is wired now (the
//! landing page reads it), and this test is what keeps the next one from happening.
//!
//! # The ratchet
//!
//! There are dozens of these in a crate this size, and deleting them all in one commit would be a refactor nobody could review. So
//! `docs/plans/orphan-queries.txt` names every orphan that exists today, and this test fails when a **new** one appears. The list
//! can only shrink; the way to shrink it is to wire the function or delete it, and the way to lie about it is to add a line —
//! which the file's own header forbids and a reviewer can see in the diff.
//!
//! # Why this test no longer measures anything
//!
//! There used to be two implementations of the measurement: `scripts/orphan-queries.py`, which writes the file, and this test,
//! which recomputed the same closure in Rust and compared. Two implementations of one measurement is how a count drifts, and on
//! 2026-11-09 they did: both decided reachability by **bare name**, so a call to any `record(` anywhere in the crate marked every
//! `record` reachable. `decision_audit::record` has no product caller at all and sat outside the list — which is why the
//! operator's audit route, `GET /decisions/audit`, always answers an empty trail for decisions nobody records.
//!
//! The script now compares on `module::name`, counts `use darkhold_db::module;` imports as evidence, and computes per-function
//! edges from that function's own body. Correcting the measurement moved the baseline from 43 to 57: the debt was not new, the
//! ruler was. This test asks the one implementation whether the file is current.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/app -> crates -> workspace")
        .to_path_buf()
}

/// The allowlisted names, comments dropped.
fn allowlisted(root: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(root.join("docs/plans/orphan-queries.txt"))
        .expect("docs/plans/orphan-queries.txt");
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.split_whitespace().next().unwrap_or(line).to_owned())
        .collect()
}

/// Every `pub async fn` declared under `crates/db/src`.
fn declared(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.join("crates/db/src")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read crates/db/src") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().is_none_or(|extension| extension != "rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            for line in text.lines() {
                let Some(rest) = line.trim_start().strip_prefix("pub async fn ") else {
                    continue;
                };
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if !name.is_empty() {
                    out.push(name);
                }
            }
        }
    }
    out
}

#[test]
fn no_new_database_function_is_reachable_only_from_a_test() {
    let root = workspace_root();
    let output = Command::new("python3")
        .arg("scripts/orphan-queries.py")
        .arg("--check")
        .current_dir(&root)
        .output()
        .expect("run scripts/orphan-queries.py — the ratchet's only implementation");

    assert!(
        output.status.success(),
        "the orphan baseline is not current (M70-13). Run `python3 scripts/orphan-queries.py` to regenerate it, then wire or \
         delete every function it added.\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    // The scan's own sanity check, restated here because a scan that reads nothing agrees with an empty file.
    let allowed = allowlisted(&root);
    assert!(
        allowed.len() > 20,
        "the baseline holds only {} entries — the scan is not reading the tree it thinks it is",
        allowed.len()
    );
    assert!(
        declared(&root).len() > 300,
        "the declaration scan found suspiciously few functions"
    );
}

#[test]
fn the_allowlist_shrinks_rather_than_rots() {
    // A line whose function no longer exists is a stale line, and a file that can only grow stops being a ratchet.
    let root = workspace_root();
    let declared = declared(&root);
    let stale: Vec<String> = allowlisted(&root)
        .into_iter()
        .filter(|name| !declared.contains(name))
        .collect();
    assert!(
        stale.is_empty(),
        "docs/plans/orphan-queries.txt names function(s) that no longer exist: {}",
        stale.join(", ")
    );
}
