# docs/plans/m64-16-offline-pre-registration-harness.md — plan card

Source: `docs/plans/m64-16-offline-pre-registration-harness.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

recorded history on `taste::eval`'s metrics before it takes live traffic"* (`docs/requirements.csv`).
**Source of the doctrine, and what it is not.** `docs/wiki/` vendors a published AO3 recommendation
  `Cargo.toml` says so). The export is a SQL script; the harness is a pure function over its output.
From `docs/design/2026-11-01-ranking-signals-and-surfaces.md` (measured in the tree and the deployed
and the schema the rungs read (`migrations/*/0068_work_metrics.sql`, `0098_ranking.sql`,
`0129_work_ratings.sql`):
scripts/eval/01-engagement-census.sql     does the data exist? (read-only, both engines)
scripts/eval/02-export-set.sh             → eval/eval-set.json   (temporal split, pseudonymised)
crates/taste/src/ladder.rs                pure scoring over that file (unit-tested)
crates/taste/examples/ladder.rs           the CLI that prints the table and exits non-zero
### 1. `crates/taste/src/eval.rs` — three metrics the study's table needs
**Verification** (in `eval.rs`'s `#[cfg(test)] mod tests`, beside the existing ones):

## Copy-pasteable code / commands

library_items                                  141,317
library_items carrying any endorsement           1,247   (0.9 %)
rows where views is non-null                         0
engaged rows that join a work_tag                    0   ← the tag rung's blocker
reading_progress rows                                1
work_ratings rows                                    0

scripts/eval/01-engagement-census.sql     does the data exist? (read-only, both engines)
scripts/eval/02-export-set.sh             → eval/eval-set.json   (temporal split, pseudonymised)
crates/taste/src/ladder.rs                pure scoring over that file (unit-tested)
crates/taste/examples/ladder.rs           the CLI that prints the table and exits non-zero

/// `precision@k`: the share of the first `k` *slots* that hit a relevant work.
///
/// The denominator is `k`, never the number of works the ranking actually held. A four-item
/// ranking with four hits is `4/20 = 0.2`, not `1.0`: it served a fifth of a page and a
/// caller reading 1.0 would believe it served the page. This is the same rule as
/// [`overlap_at`], and for the same reason.
#[must_use]
pub fn precision_at(ranking: &[String], judged: &[Judgement], k: usize) -> f64 {
    if k == 0 {
        return 0.0;
    }
    let relevant: HashSet<&str> = judged
        .iter()
        .filter(|j| j.grade > 0)
        .map(|j| j.work_id.as_str())
        .collect();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut hits = 0_usize;
    for work in ranking.iter().take(k) {
        if !seen.insert(work.as_str()) {
            continue; // a repeat occupies a slot and earns nothing
        }
        if relevant.contains(work.as_str()) {
            hits += 1;
        }
    }
    hits as f64 / k as f64
}

/// `F1@k` — the harmonic mean of [`precision_at`] and [`recall_at`].
///
/// `None` when the reader has no relevant judgements, because a reader who judged nothing is
/// not a reader who was served perfectly (the rule [`recall_at`] already follows).
#[must_use]
pub fn f1_at(ranking: &[String], judged: &[Judgement], k: usize) -> Option<f64> {
    let recall = recall_at(ranking, judged, k)?;
    let precision = precision_at(ranking, judged, k);
    if precision + recall == 0.0 {
        return Some(0.0);
    }
    Some(2.0 * precision * recall / (precision + recall))
}

/// `AP@k` — average precision, the ordering term F1 cannot see.
///
/// The denominator is the number of relevant works, **never** `min(k, relevant)` and never the
/// list's length: clamping it lets a three-item list score 1.0 on an eleven-work judgement set.
#[must_use]
pub fn average_precision_at(ranking: &[String], judged: &[Judgement], k: usize) -> Option<f64> {
    let relevant: HashSet<&str> = judged
        .iter()
        .filter(|j| j.grade > 0)
        .map(|j| j.work_id.as_str())
        .collect();
    if relevant.is_empty() || k == 0 {
        return None;
    }
    let mut seen: HashSet<&str> = HashSet::new();
    let mut hits = 0_usize;
    let mut total = 0.0;

## Verification command and expected output

**Verification** (in `eval.rs`'s `#[cfg(test)] mod tests`, beside the existing ones):
cargo test -p darkhold-taste --lib eval -- --nocapture
Expected: every existing test still green, plus:
test eval::tests::precision_divides_by_k_not_by_the_list ... ok
test eval::tests::a_repeat_earns_one_slot_and_no_second_hit ... ok
test eval::tests::f1_is_none_when_nothing_was_judged ... ok
test eval::tests::a_short_list_cannot_reach_a_perfect_map ... ok
test result: ok. 13 passed; 0 failed
(Nine tests exist in the module today — `grep -c '#\[test\]' crates/taste/src/eval.rs` — and the
| test | case | expected |
cargo test -p darkhold-taste --lib ladder
Expected:
test ladder::tests::a_group_collapse_is_a_guardrail_not_a_zero ... ok
test ladder::tests::a_reader_without_judgements_is_counted_not_averaged ... ok
test ladder::tests::a_refused_rung_is_not_a_zero ... ok
test ladder::tests::an_unknown_challenger_names_itself ... ok
test ladder::tests::coverage_counts_distinct_works_not_slots ... ok
test ladder::tests::the_bar_is_a_conjunction ... ok
test ladder::tests::the_cut_and_the_metrics_are_pinned ... ok
test ladder::tests::the_ratio_is_measured_against_popularity ... ok

## RED/GREEN evidence

can be measured on. This harness is therefore built to run **green on a thin corpus by printing the
Expected: every existing test still green, plus:
test result: ok. 13 passed; 0 failed
four above make thirteen. The count is in the expectation because "the suite is green" on a filtered
`LadderReport::verdict(challenger)` is the conjunction, and it names which limb failed:
test result: ok. 8 passed; 0 failed
a rung refused or the bar failed (**a refusal is a non-zero exit**, because a run that could not
it would have died the same way *and exited 0*, because `psql` does not stop on a failed statement
`json_object` unless it is wrapped in `json()`, and **`psql` exits 0 after a failed statement** — the
| 1 | `precision_at`/`f1_at`/`average_precision_at` in `crates/taste/src/eval.rs` + 4 tests | `cargo test -p darkhold-taste --lib eval` | `13 passed` | done |
| 2 | `crates/taste/src/ladder.rs` + 8 tests | `cargo test -p darkhold-taste --lib ladder` | `8 passed` | done |
  `implemented-locally-tested` when steps 1–3 are green, never before.
