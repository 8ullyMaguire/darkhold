# Suki crosswalk intake — parked candidates, 2026-11-26

This file is the parking plan for the eight candidates in
`docs/spec-amendments/suki-crosswalk-2026-11-26.md`. Nothing here is in the open queue and nothing
here is implemented. Per the standing rule, an item gets an execution plan with exact code and
per-step verification **after** its gate clears; this file fixes the gate, the target section and
the acceptance command now so the next session does not re-derive the triage.

Ranking rule: extends an existing mechanism → serves the north star or a decision's correctness →
has live supply. Gated items come after ungated ones.

## XW-1 — Explicit ranking feedback and a side-by-side arm comparison

- **Extends:** `M64-01`…`M64-16` (not §29.2's content arena, not `taste::eval`).
- **Gate:** `experiment_buckets > 0` in `[discovery]` and at least one arm split on one surface.
  Measured 2026-11-26: no `[discovery]` section in
  `/personal/documents/code/rust/darkhold/darkhold.toml`; 0 assignments, 0 snapshots, 0 decisions.
- **First action (ops, not code):** add `[discovery]` with `experiment_buckets = 2` to that file,
  restart the user unit, then verify with
  `ssh thinkcentre 'curl -sS http://127.0.0.1:8081/api/v1/recommendation/surfaces'`.
  Expected: the same surface list with two named algorithms where the arm is configured; if the
  response is unchanged or the port refuses, stop and record the failure — do not fabricate a pass.
- **Code target when unblocked:** `crates/app/src/routes/experiment_sweep.rs` and the operator
  experiment console behind `M64-15`; a new feedback table joined by `(surface, arm, pseud)`, never
  a new Elo arena. Keep one comparison per reader per query hash and cap per-user weight.
- **Acceptance:** on one surface, an explicit "good order / bad order" and a two-pane comparison of
  the same query produce rows attributed to an arm; a decision reads those rows only after the
  existing `M64-04` sample, duration and guardrail conjunction clears. `cargo test -p darkhold-db
  --test experiment_snapshots` and `cargo test -p darkhold-app --test m64_15_preview` stay green;
  a mutant that lets one reader promote an arm must fail a named test.
- **Watch-out:** §47.4 keeps explicit feedback out of earned taste signals; §28.10's promotion ban is
  about roadmap cards, not algorithms.

## XW-2 — Reader-facing identification queue

- **Extends:** §15.1 character assertions and §15.17 auto-created canonical entities.
- **Gate:** met — 8,469 `work_characters` rows live. A two-reader quorum still needs two active
  reviewers (24 accounts total), so measure reviewers before choosing the quorum.
- **Code target:** `crates/app/src/routes/characters.rs`, `crates/db/src/work_characters.rs`, and the
  existing canonical-entity proposal path; add a candidate list and a POST that records a proposal,
  never an auto-applied tag.
- **Acceptance:** `cargo test -p darkhold-app --test m70_13_character_authoring` and
  `cargo test -p darkhold-db --test work_characters_store` stay green; a new double-engine test
  proves one candidate accepted by a quorum writes one proposal row and zero canonical writes until
  a human applies it.
- **Watch-out:** no new vote role; reuse the proposal tables and §15.17's review state.

## XW-3 — Named, switchable negative exclusion sets

- **Extends:** §7.5's scoped mutes and §15.7's filters. This is covered-but-weaker, not new machinery.
- **Gate:** `work_tag_votes = 0`; start from the reader's own mute rows, not learned negatives.
- **Code target:** the reader's filter settings already exposed through §46; add a named set with one
  active-at-a-time semantics and a single resolve point used by every surface.
- **Acceptance:** a two-engine test proves set A and set B produce different orders for one reader
  and the same reachable set under `for-you` and `new` (§43.3's permutation rule), with the chosen
  set private to its reader.
- **Watch-out:** never an invisible filter; a set either names an explicit logged exclusion or acts
  as a ranking nudge that leaves the candidate set unchanged.

## XW-4 — Reviewer calibration against audited ground truth

- **Extends:** §19 trust/quorum/appeals and §34 calibrated decision services.
- **Gate:** not met — `review = 0` and `review_classifications = 0`. Seed a small audited set first.
- **Acceptance:** with a seeded audited set, an aggregate agreement number is published beside its
  denominator; `cargo test -p darkhold-app --test m70_13_leakage_review_writer` stays green.
- **Watch-out:** process feedback stays advisory (§28.10); no score may determine trust authority.

## XW-5 — "Garden" order over least-described works

- **Extends:** §15.6 metadata completeness and `M69-20`'s tag script.
- **Gate:** met in principle (completeness data exists); default-off decision required because it
  changes what a stranger sees.
- **Code target:** join §43.2's one sort vocabulary; no new surface and no new ranking subsystem.
- **Acceptance:** an opt-in `sort=` value returns works ordered by missing metadata, both engines,
  and the reachable set is identical to `new` (§43.3). It never appears in `for-you`.
- **Watch-out:** a work queue for the operator is not a reader-facing quality signal.

## XW-6 — Rebuild lock and drift check on decision inputs

- **Extends:** §10's worker and `M64-03`'s daily snapshots, which promotions read.
- **Gate:** not met as a bug — single worker today. Add when a second worker or manual re-run can
  overlap the nightly job.
- **Acceptance:** a concurrent-rebuild test fails when two writers can produce two snapshot rows for
  one `(surface, arm, day)`; a drift check asserts cached aggregates against a direct recount.
- **Watch-out:** do not invent an invariant nobody has observed breaking; this is insurance.

## XW-7 — Saved Filter revision history and simple insights

- **Extends:** §14.2 (`versioned_query_ast` exists; a readable revision log and open/return counts do not).
- **Gate:** not met — `saved_views = 0` and saved-view subscriptions = 0. Surface the mechanism
  before adding history to it.
- **Code target:** `crates/app/src/routes/saved_filters.rs` and `crates/db/src/saved_filter_life.rs`.
- **Acceptance:** `cargo test -p darkhold-app --test m69_21_saved_filters` and
  `cargo test -p darkhold-db --test saved_filter_life_two_engines` stay green; a new test proves an
  edit appends a readable revision and the counts stay private to the filter's owner.
- **Watch-out:** keep the spec's vocabulary — §14.2 explicitly refuses the word *lens*.

## XW-8 — Source-endorsement priors for taste cold start

- **Extends:** `M63-37`'s per-source engagement storage and `M64-16`'s offline ladder.
- **Gate:** supply present but thin — 1,247 of 141,317 library rows carry an endorsement.
- **Acceptance:** a source's kudos is stored as a prior with provenance, and a test proves it never
  changes an interaction's earned/incentivized kind (§47.4).
- **Watch-out:** a source endorsement is a fact about a work, never a fact about a reader (§0.3).

## Retire or promote

When an item is promoted, add its requirement id to `docs/requirements.csv` with evidence and gates,
fold the wording into the spec section named above, move it out of this file, and record the move in
the vault crosswalk note. A refused item gets its reason written into the spec amendment's
"Deliberately not transferred" section, not deleted — an unrecorded refusal gets re-proposed.
