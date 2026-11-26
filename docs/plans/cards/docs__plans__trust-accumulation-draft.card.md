# docs/plans/trust-accumulation-draft.md — plan card

Source: `docs/plans/trust-accumulation-draft.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

`docs/spec.md`, `docs/requirements.csv`, or any code. It exists for review only.
`docs/plans/remaining-work.md`.
grep hits are `set_trust_proxy`, an unrelated function in `server.rs` that
trust setter are `crates/app/tests/milestone_14.rs:359` and
`crates/app/tests/milestone_22.rs:641` - both tests.
`crates/db/src/instance_theme.rs`). Per-user taste is
| score arithmetic | new `crates/domain/src/trust.rs` - pure, no IO, fully unit-testable |
| persistence | extend `crates/db/src/governance.rs` |
- **M57-01** - `crates/domain/src/trust.rs` plus its unit tests. No schema, no
- **M57-02** - migration and `governance.rs` persistence, both dialects.

## Copy-pasteable code / commands

gain = base_gain_bp * multiplier * taste_factor     (per qualifying event)
loss = base_loss_bp * multiplier * penalty_factor    (per qualifying event)

taste_factor = clamp(1.0 + alignment * 0.5, 0.5, 1.5)

## Verification command and expected output

Grepping every call site returns test harnesses only. Most of the `set_trust`
loss rates, multipliers and a taste gate is not "adding a feature to a working
rewriting every gate to join a second table is churn for no benefit. One writer
per table, paired, with a test asserting the two never disagree.
The floor is a promotion gate only. It never blocks reading, posting, or
| A published comment surviving review | gain | first-time pass only, not every comment |
database access, because that is the only way to test the asymmetry property
   *floor* - a faster earn rate for aligned users is a nudge, a promotion gate

## RED/GREEN evidence

| Reviewer quorum approval of a proposal | gain | the proposal passed, not the proposer voting |
