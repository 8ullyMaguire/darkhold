# docs/plans/m45-23-north-star.md — plan card

Source: `docs/plans/m45-23-north-star.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

`docs/requirements.csv` M45-23:
`docs/spec-gaps-design-review-2026-09-22.md` line 474 lists as item 5 of the priority order
selection-probability logging", `implemented-fully-tested`, and migration `0098_ranking.sql`
record which mechanism surfaced the work.** `crates/db/src/recommendation_slots.rs` has no
**File:** `crates/db/src/north_star.rs` (new)
**File:** `crates/app/tests/north_star.rs` (new)

## Copy-pasteable code / commands

pub struct NorthStar {
    pub works_rated_per_month: f64,
    pub median_days_to_find: Option<f64>,
    pub rated_works: i64,
    pub loved_works: i64,
    pub unattributed: i64,       // loved, no slot row
    pub by_mechanism: Vec<MechanismAttribution>, // key, loved_works, share
    pub missing_inputs: Vec<MissingInput>,      // see 53.5
}

cd ~/code-local/rust/darkhold && export PATH="$HOME/.cargo/bin:$PATH"
cargo fmt --all
cargo clippy -p darkhold-db -p darkhold-app --all-targets   # must print nothing
cargo test -p darkhold-db -p darkhold-app --no-fail-fast -- --test-threads=4
DARKHOLD_TEST_PG_URL='postgres://postgres:smoke_pw@127.0.0.1:5432/postgres' \
  cargo test -p darkhold-db -p darkhold-app --no-fail-fast -- --test-threads=4

## Verification command and expected output

Cases that will not pass by accident:
2. **The earliest slot wins.** Seed one work served twice, rated after both. Assert the
4. **`median_days_to_find` is `None`, not `0.0`, with no completed pair.** §53.5. Assert the
   JSON has `null`, not `0` — a test on the Rust type alone would miss a serialiser that
## Verify the whole step
cargo test -p darkhold-db -p darkhold-app --no-fail-fast -- --test-threads=4
  cargo test -p darkhold-db -p darkhold-app --no-fail-fast -- --test-threads=4
**Do not run a mutation harness concurrently, and do not `git add -A` while one runs.** A gate
edits the source under test; a concurrent `cargo test` compiles mutated source and reports the
gate's intended RED as a failure, and a broad add stages whichever mutation is applied at that

## RED/GREEN evidence

gate's intended RED as a failure, and a broad add stages whichever mutation is applied at that
