# docs/plans/m45-phase1-taste-signal.md — plan card

Source: `docs/plans/m45-phase1-taste-signal.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

Status: **live build plan for `docs/spec.md` §49's first three rows.** M45-16,
`docs/verification.md` for the two bugs it turned up). **Step 4 (history import)
`crates/db/src/tasting.rs` with 14 unit tests, and **nothing called them** — so
`crates/app/src/routes/discovery.rs` builds `tags_by_work` from
`crates/db/src/tag_confirmation.rs` with eight unit tests.
red because `scripts/check-snapshot-pii.py` requires a decision for every column
adding them to `docs/snapshot-column-policy.json` with real reasoning, not by
**11 pool timeouts.** All 11 panicked in `test-support/src/lib.rs:319` — the
  --> crates/app/src/crypto.rs:60:5
at one version can only mean two builds. `~/.cargo/config.toml` sets a **global**
`.cargo/config.toml` now sets `[build] target-dir = "target"`. `aria`, `ficnexus`
`test-support/src/lib.rs:319` with no assertion behind them, at load average 24–40

## Copy-pasteable code / commands

cargo test -p darkhold-db --lib tag_confirmation
# expect: unconfirmed tags contribute 0; capped at N; inaccurate contributes 0
DARKHOLD_TEST_PG_URL='postgres://postgres:smoke_pw@127.0.0.1:5432/postgres' \
  cargo test -p darkhold-db --lib tag_confirmation
# expect: same counts, or the migration did not declare both dialects
cargo test -p darkhold-db --lib dialect_parity -- --nocapture
# expect: the two dialects declare the same columns and indexes

cargo test -p darkhold-app --test m29_transparency --no-fail-fast
# expect 38 passed, 0 failed — the existing control must still hold

error: found crates (`hex` and `hex`) with colliding StableCrateId values
  --> crates/app/src/crypto.rs:60:5
error: doctest failed, to rerun pass `-p darkhold-app --doc`

## Verification command and expected output

SQLite and PostgreSQL 15, both exit 0 — see the 2026-10-01 section of
index downgraded to a plain one — and each got the test written for it.
Each step is: migration in **both** dialects → code → tests → two-backend gate.
Verify after the step:
cargo test -p darkhold-db --lib tag_confirmation
  cargo test -p darkhold-db --lib tag_confirmation
cargo test -p darkhold-db --lib dialect_parity -- --nocapture
cargo test -p darkhold-app --test m29_transparency --no-fail-fast
**The test that matters most here is the negative one.** A work whose tags are
| fixture | expected |
The parity test earned its keep immediately: the SQLite arm was missing the
**The snapshot gate caught migration 0099's own columns.** `cargo test -p
narrowing the gate:
This is the §11.16 gate doing the job it was built for, on the same day the
columns landed. Worth noting because the alternative was to run the gate after
**The existing taste test went red, and that was the filter working.**
the padding sorts first, which is the situation the test is about. It now fails
### Gate at `f90fc9e`
| backend | passed | failed | exit |
Both green, at `--test-threads=2`.

## RED/GREEN evidence

**Steps 1, 2 and 3 are built and green on both engines.** M45-16 is
verified red by reinstating the defect. Two further mutations survived the first
No step starts until the previous one is committed green.
# expect 38 passed, 0 failed — the existing control must still hold
red because `scripts/check-snapshot-pii.py` requires a decision for every column
**The existing taste test went red, and that was the filter working.**
`a_signed_in_readers_feed_is_ordered_by_taste` failed the moment the route read
**One of the new unit tests was green against broken code.**
for real ones; `r` < `u`, so the real tags sorted first and the assertion passed
| backend | passed | failed | exit |
Both green, at `--test-threads=2`.
Getting there took two runs that were **not** red for a code reason, and both are
