//! One producer of a reader's taste signal, and it is wired (M70-13).
//!
//! The gravity layer reads a number called `taste_signal` and mixes it with the surface's base score. When two modules can
//! produce that number they will eventually disagree, and the disagreement has no error: the page renders, the ranking looks
//! plausible, and the reader is served a different answer on two routes.
//!
//! This file is the invariant that keeps it one producer. It scans the store crate for any symbol named `taste_signal*`
//! outside `taste_signal.rs`, and asserts that the module the app calls is the one that owns the name. It is the same shape as
//! `orphan_queries.rs`: a rule about the codebase, checked by reading it, because a unit test on a function nobody calls is
//! evidence of nothing.

use std::fs;
use std::path::{Path, PathBuf};

fn db_src_dir() -> PathBuf {
    // `CARGO_MANIFEST_DIR` is `crates/app`, and the store crate is its sibling.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .join("db/src")
}

/// Every `taste_signal*` definition in the store crate, as `(file, symbol)`.
fn taste_signal_symbols(dir: &Path) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(path) = stack.pop() {
        let Ok(entries) = fs::read_dir(&path) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            for line in text.lines() {
                let line = line.trim_start();
                let Some(rest) = line
                    .strip_prefix("pub async fn ")
                    .or_else(|| line.strip_prefix("pub fn "))
                else {
                    continue;
                };
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if name.starts_with("taste_signal") {
                    let file = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or_default()
                        .to_owned();
                    found.push((file, name));
                }
            }
        }
    }
    found.sort();
    found
}

#[test]
fn the_taste_signal_has_exactly_one_home() {
    let symbols = taste_signal_symbols(&db_src_dir());
    let strays: Vec<_> = symbols
        .iter()
        .filter(|(file, _)| file != "taste_signal.rs")
        .collect();
    assert!(
        strays.is_empty(),
        "a taste-signal producer outside taste_signal.rs is a second answer to the same question: {strays:?}"
    );
}

#[test]
fn the_gravity_pull_actually_applies_the_signal() {
    // The first version of this test asserted that *some* app file calls `taste_signal::for_account`, and a mutant that deleted
    // the discovery route's call **survived** — concierge calls the same function, so "a caller exists" stayed true while the
    // page that pulls on the signal stopped pulling. The mutant found a weak test, which is what mutants are for.
    //
    // What the M64-09 fix added is not a call, it is an assignment: each candidate's `taste_signal` is set from the reader's
    // shelf before the mixture runs. If that assignment is gone the function is still called, still returns signals, and the
    // page is inert again — so the assignment is what the guard has to look for.
    let discovery = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/routes/discovery.rs");
    let text = fs::read_to_string(&discovery).expect("the discovery route");
    assert!(
        text.contains("candidate.taste_signal = signals"),
        "the gravity pull must assign the shelf signal to the candidate, not merely fetch it"
    );
    assert!(
        text.contains("taste_signal::for_account"),
        "and the page must fetch it from the one producer"
    );
}
