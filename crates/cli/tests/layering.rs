//! `M63-28` — `dh` is a client of `/api/v1`, and it may not reach into the server's crates.
//!
//! The placement decision (`docs/plans/cli-placement.md`) says the CLI owns no durable data beyond a
//! cache, so it belongs beside the contract it consumes and **not** next to the implementation. The
//! rule was true when the crate was created and nothing enforced it: `Cargo.toml` happened to list
//! only `darkhold-domain`, and one `cargo add darkhold-db` later — for a convenient helper, in a
//! hurry — the rule would be false and every test would stay green.
//!
//! This is the same shape as the workspace layering gates: the manifest *is* the claim, so the test
//! reads the manifest. A dependency added through a workspace-inherited table (`dep.workspace =
//! true`) is caught too, because the name is what matters, not where it was declared.

use std::path::Path;

/// Crates the CLI must not depend on: the server's implementation.
const FORBIDDEN: &[&str] = &[
    "darkhold-app",
    "darkhold-db",
    "darkhold-taste",
    "darkhold-scrapers",
    "darkhold-metadata",
];

#[test]
fn the_cli_does_not_depend_on_the_server() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest)
        .unwrap_or_else(|e| panic!("read {}: {e}", manifest.display()));

    // Only the dependency tables that ship: `[dev-dependencies]` would still be a dependency on the
    // server in the test graph, but it cannot leak into the binary, so the rule is about the tables
    // a release build reads.
    let must_not: Vec<&str> = FORBIDDEN
        .iter()
        .copied()
        .filter(|krate| {
            text.lines().any(|line| {
                let line = line.trim_start();
                !line.starts_with('#')
                    && (line.starts_with(&format!("{krate} "))
                        || line.starts_with(&format!("{krate}="))
                        || line.starts_with(&format!("{krate}.workspace")))
            })
        })
        .collect();

    assert!(
        must_not.is_empty(),
        "the CLI depends on {must_not:?}, which means the binary now ships the server's \
         implementation: a CLI that links the app crate can no longer be built against a remote \
         instance, and every rule it needs is answered twice (docs/plans/cli-placement.md)"
    );
}

#[test]
fn the_manifest_was_actually_read() {
    // A test that asserts an absence passes on an empty file. The control is the one dependency the
    // CLI is *supposed* to have: the shared vocabulary from `darkhold-domain`.
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest).expect("read Cargo.toml");
    assert!(
        text.contains("darkhold-domain"),
        "darkhold-domain is the CLI's one workspace dependency (the metric vocabulary is shared, \
         not re-implemented); finding neither it nor a dependency table means this file was not read"
    );
}
