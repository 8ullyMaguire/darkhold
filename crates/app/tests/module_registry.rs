//! §59.3 — the module registry's central claim, enforced.
//!
//! `crates/domain/src/modules/registry.rs` says in its own header:
//!
//! > Every module mounted through `module_mount` in `server.rs` has a row here, and
//! > `m59_extensions::every_route_module_is_registered` fails when the two disagree — a new surface
//! > cannot be added outside the mechanism.
//!
//! **That test did not exist.** `grep -rn every_route_module_is_registered crates/` found the sentence
//! and nothing else, so for as long as the registry has existed a surface could be mounted with no
//! row and no gate would have said so. This file is the test the header promises.
//!
//! It reads `server.rs` as text, because the thing under test is the *agreement between two files*:
//! the mount sites are code, not data, and reading them is the only way to see them. Every count is
//! asserted for the same reason — a parser that stops matching (a reformatted call, a renamed helper)
//! would leave this gate green while it checked nothing, which is the repository's oldest trap.
//!
//! The registry holds **two kinds of row**, and the rules differ because the kinds differ:
//!
//! * `Stability::Stable` — a compiled surface. It must be mounted, and it may be on by default.
//! * `Stability::Planned` — a named slot the project intends to fill (`story.similar`,
//!   `home.trending`, …). It must **not** be mounted and must **not** be on by default: a planned
//!   surface that is mounted is lying about being planned, and one that is on by default would be
//!   advertised to readers who cannot reach it.
//!
//! The first version of this file asserted "no id is mounted twice" and "every row is mounted", and
//! both were wrong for reasons worth keeping:
//!
//! * a module may legitimately be mounted at several prefixes (`roadmap` has three, `monetization`
//!   three), so duplicate mount ids are the design, not a fault;
//! * the 23 dotted rows are planned slots, so "every row is mounted" would have forbidden the very
//!   thing the registry uses `Stability::Planned` for.
//!
//! A gate written from the header sentence alone would have failed 23 true rows and 10 true mounts
//! and been "fixed" by deleting rows. The rows were right; the rule was not.

use std::collections::BTreeSet;
use std::path::Path;

use darkhold_domain::modules::registry::{CORE, MODULES};
use darkhold_domain::modules::{Stability, Toggle};

/// The number of `module_mount` call sites in `server.rs` when this test was written.
///
/// Pinned so a parse that matches nothing fails instead of passing vacuously. Bumping it is part of
/// adding a surface, and the failure message says so.
///
/// 100, not 97: `search`, `archive` and `succession` gained mounts after the last bump and the
/// number was never moved, so this test had been failing since those three landed — measured at
/// `5c397f3a`, so it is not a wrap-up of the `cargo fmt` that rewrote the calls onto their own
/// lines (`mounted()` parses `module_mount(` then the next quoted string, so wrapping is invisible
/// to it; the sibling walk in `m65_module_host.rs` was not, and did fail on the wrapping).
const MOUNT_SITES: usize = 100;

/// The number of rows in `MODULES` when this test was written, split by stability.
///
/// 81 stable rows, not 79: `archive` and `succession` are registry rows now (§13's account archive
/// and §11.11a), and the index above says so. `search` was already a row and gained only a mount.
const STABLE_ROWS: usize = 81;
const PLANNED_ROWS: usize = 23;

fn server_source() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/server.rs");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Every `module_mount(&state, "id", …)` id in mount order, duplicates kept.
fn mounted() -> Vec<String> {
    let source = server_source();
    let mut ids = Vec::new();
    let mut rest = source.as_str();
    // Hand-rolled rather than a regex dependency: one pattern, and the workspace already parses its
    // sources in Python for the spec gate.
    while let Some(at) = rest.find("module_mount(") {
        rest = &rest[at + "module_mount(".len()..];
        let Some(q1) = rest.find('"') else { break };
        let after = &rest[q1 + 1..];
        let Some(q2) = after.find('"') else { break };
        let id = &after[..q2];
        if !id.is_empty()
            && id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            ids.push(id.to_owned());
        }
        rest = &after[q2..];
    }
    ids
}

#[test]
fn every_mount_site_is_core_or_a_stable_row() {
    let ids = mounted();
    assert_eq!(
        ids.len(),
        MOUNT_SITES,
        "expected {MOUNT_SITES} module_mount sites in server.rs and found {} — if a surface was \
         added or removed, update MOUNT_SITES; if the call shape changed, fix the parser (a gate \
         that matches nothing passes)",
        ids.len()
    );

    let stable: BTreeSet<&str> = MODULES
        .iter()
        .filter(|m| m.stability == Stability::Stable)
        .map(|m| m.id)
        .collect();
    let planned: BTreeSet<&str> = MODULES
        .iter()
        .filter(|m| m.stability == Stability::Planned)
        .map(|m| m.id)
        .collect();
    let core: BTreeSet<&str> = CORE.iter().copied().collect();

    let missing: Vec<&str> = ids
        .iter()
        .map(String::as_str)
        .filter(|id| !stable.contains(id) && !core.contains(id))
        .collect();
    assert!(
        missing.is_empty(),
        "these surfaces are mounted with no Stable Module row: {missing:?} — add a row (§59.3), or \
         add the id to CORE if it is one of the five permission-bearing surfaces that have no row \
         by design"
    );

    let mounted_but_planned: Vec<&str> = ids
        .iter()
        .map(String::as_str)
        .filter(|id| planned.contains(id))
        .collect();
    assert!(
        mounted_but_planned.is_empty(),
        "these rows say `Stability::Planned` while server.rs mounts them: {mounted_but_planned:?} \
         — promote the row to Stable, because a mounted surface is not planned"
    );
}

#[test]
fn every_stable_row_is_mounted() {
    let ids: BTreeSet<String> = mounted().into_iter().collect();
    let orphaned: Vec<&str> = MODULES
        .iter()
        .filter(|m| m.stability == Stability::Stable)
        .map(|m| m.id)
        .filter(|id| !ids.contains(*id))
        .collect();
    assert!(
        orphaned.is_empty(),
        "these Stable rows describe surfaces that server.rs does not mount: {orphaned:?} — the \
         module list would advertise a feature that does not exist"
    );
}

#[test]
fn planned_rows_are_off_by_default_and_plannable() {
    let planned: Vec<_> = MODULES
        .iter()
        .filter(|m| m.stability == Stability::Planned)
        .collect();
    assert_eq!(
        planned.len(),
        PLANNED_ROWS,
        "the planned-slot count moved; update PLANNED_ROWS with the reason"
    );

    let advertised: Vec<&str> = planned
        .iter()
        .filter(|m| m.default_on)
        .map(|m| m.id)
        .collect();
    assert!(
        advertised.is_empty(),
        "planned rows must not be on by default, or they are advertised to readers who cannot \
         reach them: {advertised:?}"
    );

    // A planned row with `Toggle::Fixed` can never be switched on, so it would stay unbuilt forever
    // even after someone wrote the surface — the row has to be promotable.
    let unpromotable: Vec<&str> = planned
        .iter()
        .filter(|m| matches!(m.toggle, Toggle::Fixed))
        .map(|m| m.id)
        .collect();
    assert!(
        unpromotable.is_empty(),
        "planned rows need a switch so they can be turned on once built: {unpromotable:?}"
    );
}

/// A row whose `default_on` is false must be reachable by a toggle, and a `Fixed` module must not be
/// `default_on: false` — a surface that is off by default *and* not switchable can never be turned on,
/// which is a dead feature with a row.
#[test]
fn a_module_that_is_off_by_default_can_still_be_turned_on() {
    let dead: Vec<&str> = MODULES
        .iter()
        .filter(|m| !m.default_on && matches!(m.toggle, Toggle::Fixed))
        .map(|m| m.id)
        .collect();
    assert!(
        dead.is_empty(),
        "off by default with no toggle, so unreachable: {dead:?}"
    );
}

#[test]
fn the_row_counts_are_what_the_index_says() {
    let stable = MODULES
        .iter()
        .filter(|m| m.stability == Stability::Stable)
        .count();
    let planned = MODULES
        .iter()
        .filter(|m| m.stability == Stability::Planned)
        .count();
    assert_eq!(stable, STABLE_ROWS, "stable row count moved");
    assert_eq!(planned, PLANNED_ROWS, "planned row count moved");
    assert_eq!(
        MODULES.len(),
        stable + planned,
        "a row has a stability this test does not know about"
    );
}
