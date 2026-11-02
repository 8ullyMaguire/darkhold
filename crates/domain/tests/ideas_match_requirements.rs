//! M65-08 — an idea row becomes a module when it is built, and the catalogue must not keep both.
//!
//! §59.8 exists because a built surface left sitting in the idea catalogue is a second backlog: two
//! lists, one of which says a feature is planned while the tree serves it. §59.7's `IDEAS` is
//! generated, so the defect does not show up as a hand-written mistake — it shows up as a **stale
//! artefact**, produced when the generator could not run or when its retirement rules did not reach a
//! row.
//!
//! That is exactly what was measured here: fourteen rows sat in `IDEAS` whose `requirements.csv` row
//! said `implemented-…`. The generator only retired by row id and by title, and a document-sourced
//! idea carries the requirement's id with its own wording — the join was available and unused. The
//! fix is in the generator, and this is the assertion that keeps it fixed.
//!
//! The CSV is read with `include_str!`, so the check compiles against the file in the tree and cannot
//! be defeated by running the tests from a different directory.

use darkhold_domain::modules::{self, Stability};

/// `docs/requirements.csv`, embedded so the test reads the repository rather than a working directory.
const REQUIREMENTS: &str = include_str!("../../../docs/requirements.csv");

/// Split one CSV line, honouring double quotes.
///
/// The requirement column contains commas and quoted prose, so `split(',')` would misread the status
/// column on exactly the rows this test cares about. Only the quoting `csv` actually emits is
/// handled: doubled quotes inside a quoted field, and nothing else.
fn fields(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' if quoted => {
                if chars.peek() == Some(&'"') {
                    current.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            }
            '"' => quoted = true,
            ',' if !quoted => out.push(std::mem::take(&mut current)),
            other => current.push(other),
        }
    }
    out.push(current);
    out
}

/// Requirement id to status, from the embedded CSV.
fn statuses() -> Vec<(String, String)> {
    let mut lines = REQUIREMENTS.lines();
    let header = fields(lines.next().expect("the CSV has a header"));
    let id_at = header
        .iter()
        .position(|name| name == "id")
        .expect("the CSV has an id column");
    let status_at = header
        .iter()
        .position(|name| name == "status")
        .expect("the CSV has a status column");
    lines
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let cells = fields(line);
            (
                cells.get(id_at).cloned().unwrap_or_default(),
                cells.get(status_at).cloned().unwrap_or_default(),
            )
        })
        .collect()
}

#[test]
fn no_idea_row_describes_a_requirement_that_is_already_built() {
    let statuses = statuses();
    assert!(
        statuses.len() > 500,
        "the CSV parsed as {} rows, which is too few to be the inventory",
        statuses.len()
    );

    let mut stale = Vec::new();
    for idea in modules::ideas() {
        let Some((id, status)) = statuses.iter().find(|(id, _)| id == idea.id) else {
            // A document-sourced idea has `idea.<slug>` for an id and no requirement row. That is the
            // normal case, not a finding.
            continue;
        };
        if status.starts_with("implemented") {
            stale.push(format!(
                "{} (\"{}\") is {status} in docs/requirements.csv but still listed as an idea",
                id, idea.name
            ));
        }
    }

    assert!(
        stale.is_empty(),
        "an idea row beside the requirement that built it is the second backlog §59.8 ends. \
         Re-run `python3 scripts/ideas_catalogue.py`:\n  {}",
        stale.join("\n  ")
    );
}

#[test]
fn every_idea_row_is_planned_and_off() {
    // §59.7: an idea has no routes to switch, so it may not default on and may not claim to be
    // stable. The generator sets this, and a hand edit is the one way it would drift.
    for idea in modules::ideas() {
        assert_eq!(
            idea.stability,
            Stability::Planned,
            "{} is in the idea catalogue but is not Planned",
            idea.id
        );
        assert!(
            !idea.default_on,
            "{} is in the idea catalogue and defaults on, which would offer a surface that has no routes",
            idea.id
        );
    }
}

#[test]
fn idea_ids_are_unique_and_disjoint_from_built_modules() {
    let mut seen = std::collections::BTreeSet::new();
    for idea in modules::ideas() {
        assert!(seen.insert(idea.id), "{} is listed twice", idea.id);
        assert!(
            modules::find(idea.id).is_some(),
            "{} is in IDEAS but not resolvable through modules::find",
            idea.id
        );
    }

    // The registry carries both: a row for a surface that serves routes today, and a row for a
    // surface that is specified and not built yet (`story.similar` and its M64 siblings are the
    // second kind, and marking them Planned is the honest state, not a defect). Only the first kind
    // may collide with an idea.
    let mut built = std::collections::BTreeSet::new();
    for module in modules::all() {
        assert!(
            built.insert(module.id),
            "{} is registered twice in the built registry",
            module.id
        );
        if module.is_planned() {
            built.remove(module.id);
        }
    }

    let overlap: Vec<&&str> = seen.intersection(&built).collect();
    assert!(
        overlap.is_empty(),
        "these ids are both a built module and an idea: {overlap:?}"
    );
}
