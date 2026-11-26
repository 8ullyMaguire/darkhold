# docs/plans/suki-crosswalk-intake-2026-11-26.md — plan card

Source: `docs/plans/suki-crosswalk-intake-2026-11-26.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

`docs/spec-amendments/suki-crosswalk-2026-11-26.md`. Nothing here is in the open queue and nothing
  `/personal/documents/code/rust/darkhold/darkhold.toml`; 0 assignments, 0 snapshots, 0 decisions.
- **Code target when unblocked:** `crates/app/src/routes/experiment_sweep.rs` and the operator
- **Code target:** `crates/app/src/routes/characters.rs`, `crates/db/src/work_characters.rs`, and the
- **Code target:** `crates/app/src/routes/saved_filters.rs` and `crates/db/src/saved_filter_life.rs`.
When an item is promoted, add its requirement id to `docs/requirements.csv` with evidence and gates,

## Copy-pasteable code / commands

No fenced command block was recorded in the source document.

## Verification command and expected output

per-step verification **after** its gate clears; this file fixes the gate, the target section and
- **Gate:** `experiment_buckets > 0` in `[discovery]` and at least one arm split on one surface.
  restart the user unit, then verify with
  Expected: the same surface list with two named algorithms where the arm is configured; if the
  response is unchanged or the port refuses, stop and record the failure — do not fabricate a pass.
  existing `M64-04` sample, duration and guardrail conjunction clears. `cargo test -p darkhold-db
  --test experiment_snapshots` and `cargo test -p darkhold-app --test m64_15_preview` stay green;
  a mutant that lets one reader promote an arm must fail a named test.
- **Gate:** met — 8,469 `work_characters` rows live. A two-reader quorum still needs two active
- **Acceptance:** `cargo test -p darkhold-app --test m70_13_character_authoring` and
  `cargo test -p darkhold-db --test work_characters_store` stay green; a new double-engine test
- **Gate:** `work_tag_votes = 0`; start from the reader's own mute rows, not learned negatives.
- **Acceptance:** a two-engine test proves set A and set B produce different orders for one reader
- **Gate:** not met — `review = 0` and `review_classifications = 0`. Seed a small audited set first.
  denominator; `cargo test -p darkhold-app --test m70_13_leakage_review_writer` stays green.
- **Gate:** met in principle (completeness data exists); default-off decision required because it
- **Gate:** not met as a bug — single worker today. Add when a second worker or manual re-run can
- **Acceptance:** a concurrent-rebuild test fails when two writers can produce two snapshot rows for
- **Gate:** not met — `saved_views = 0` and saved-view subscriptions = 0. Surface the mechanism
- **Acceptance:** `cargo test -p darkhold-app --test m69_21_saved_filters` and

## RED/GREEN evidence

  --test experiment_snapshots` and `cargo test -p darkhold-app --test m64_15_preview` stay green;
  a mutant that lets one reader promote an arm must fail a named test.
  `cargo test -p darkhold-db --test work_characters_store` stay green; a new double-engine test
  denominator; `cargo test -p darkhold-app --test m70_13_leakage_review_writer` stays green.
  `cargo test -p darkhold-db --test saved_filter_life_two_engines` stay green; a new test proves an
