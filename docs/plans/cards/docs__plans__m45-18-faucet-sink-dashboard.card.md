# docs/plans/m45-18-faucet-sink-dashboard.md — plan card

Source: `docs/plans/m45-18-faucet-sink-dashboard.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

`docs/requirements.csv` M45-18:
| `Flow` enum (Faucet/Sink/Neutral) | `crates/domain/src/flows.rs` | yes, 8 tests |
   `flows.rs` already says why: a bug in a faucet produces a *negative* amount, which is
The column name is a historical artefact. `economy.rs:173` binds the caller's `i64` straight
into it, and `preservation.rs:1279` says so in a comment. **Do not divide by 100.** This plan
| `tip:{work_id}` | `routes/monetization.rs:340` | `Spend` | **sink** |
| `{member_id}` | `preservation.rs:1177` | `Preservation` | **sink** |
| `{member_id}` | `preservation.rs:1221` | `PreservationReclaim` | **faucet** |
| `{work_id}` | `payout_store.rs:323` | `Earn` | **faucet** |
`preservation.rs:1249` already does exactly this, follow that shape.
**File:** `crates/db/src/flow_store.rs` (new)
**File:** `crates/app/src/routes/flows.rs` (new)

## Copy-pasteable code / commands

cd ~/code-local/rust/darkhold && export PATH="$HOME/.cargo/bin:$PATH"
DARKHOLD_TEST_PG_URL='postgres://postgres:smoke_pw@127.0.0.1:5432/postgres' \
  cargo test -p test-support -- --test-threads=2

pub async fn mechanisms_in_window(db: &Database, since: &str, until: &str)
    -> Result<Vec<Mechanism>>

{ "faucet_credits": 0, "sink_credits": 0, "net_credits": 0, "undeclared": 0,
  "mechanisms": [ { "key": "tips", "flow": "sink", "net_credits": 0, "declared": true } ] }

## Verification command and expected output

**Verify:**
  cargo test -p test-support -- --test-threads=2
Expected: migrations apply on both engines, no checksum error.
   prefix.** Seed both for one member; assert one lands in `sink_credits` and the other in
   `faucet_credits`. This is the test that proves the derivation, and an implementation that
   grouped on the raw `reference` cannot pass it.
2. **A negative faucet stays a faucet.** Seed a faucet with a negative `amount_bp`; assert it
   is still `faucet` and lands in `faucet_credits`. This is the test that proves rule 1, and
   is included. Both endpoints inclusive, and the test says so.
Cases 0, 2, 3 and 6 will not pass by accident.
## Verify the whole step
cargo test -p darkhold-db -p darkhold-app --no-fail-fast -- --test-threads=4
  cargo test -p darkhold-db -p darkhold-app --no-fail-fast -- --test-threads=4
while one runs.** A gate edits the source under test; a concurrent `cargo test` compiles
mutated source and reports the gate's intended RED as a failure, and a broad add stages

## RED/GREEN evidence

mutated source and reports the gate's intended RED as a failure, and a broad add stages
