# docs/plans/m45-51-subject-access-and-erasure.md — plan card

Source: `docs/plans/m45-51-subject-access-and-erasure.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

Row: `docs/requirements.csv` `M45-51`, status `planned`, note "Gaps review G1. Profiling
**The erasure graph is measured, not assumed.** Parsing `migrations/sqlite/*.sql` for every
population it could not see; `crates/db/src/erasure.rs` now classifies each one and a test refuses a
**The subject-access side does not exist.** `exports.rs` is an *admin* export job with
`analytics.rs` scope vocabulary already contains `OwnResonanceLabel`, `OwnReadingBasic`,
`analytics.rs:876` `FORBIDDEN_SCOPE_NAMES` names 13 things that must never be disclosed —
   NULL. **`api_tokens` already cascades** (`0001_identity.sql`, `ON DELETE CASCADE`), so
| `crates/db/src/erasure.rs` | `subject_data`, `plan_erasure`, `erase_account` |
| `crates/app/src/routes/erasure.rs` | `GET /me/data`, `POST /me/erasure` |
| `crates/app/tests/erasure.rs` | integration, both engines |
| `crates/app/tests/route_inventory.rs` | two `ROUTE_TABLE` rows, `Audience::Authenticated` |
`crates/db/src/erasure.rs` with `subject_data`, `plan_erasure`, `erase_account`.

## Copy-pasteable code / commands

No fenced command block was recorded in the source document.

## Verification command and expected output

population it could not see; `crates/db/src/erasure.rs` now classifies each one and a test refuses a
1. **Verify intent.** The request body carries `confirm_handle`, which must equal the
   already hold a grant in memory, but no test can observe it through the database, and the
   test file says so rather than implying coverage it does not have.
could ship. So the test enumerates all 13 forbidden names, asserts none appears anywhere in
the response body as a key, and that is a standing test rather than a one-off.
- **Both engines.** The erasure test must run on PostgreSQL, because the cascade is enforced
  by the database and SQLite's FK enforcement settings differ. A cascade test that only runs
  on SQLite is a cascade test that has not been tested.
  test red; removing the `confirm_handle` check must turn exactly one red. Restored
  The 71-table measurement above is a *specification*; the test is a fact about one account.
Each step names the exact command and its expected output, because a plan an LLM cannot
`cargo test -p darkhold-app --test erasure --no-run` → compiles, 0 tests run.
**Step 2 — verify the failure is real.**
`cargo test -p darkhold-app --test erasure 2>&1 | grep 'test result'`
`cargo test -p darkhold-app --test erasure` → `ok. 3 passed`.
   cargo test -p darkhold-app --test erasure -- --test-threads=1`
**`--test-threads=1` is required on PostgreSQL for this suite**: at 2 threads the concurrent
`cargo test -p darkhold-app --test route_inventory`
**Step 6 — the forbidden-name test.**

## RED/GREEN evidence

  test red; removing the `confirm_handle` check must turn exactly one red. Restored
→ `ok. 0 passed; 3 failed`. **If it says 3 passed, the tests are not testing the store and
`cargo test -p darkhold-app --test erasure` → `ok. 3 passed`.
→ `ok. 3 passed`.
→ `ok. 3 passed`. Without the two rows it fails with
| 1 | cancellation `state IN ('queued','running')` → `state = 'never-matches'` | 1 red: `erasure_leaves_no_export_a_worker_could_still_deliver` |
| 2 | PG bookmarks arm gains `AND 1 = 0` | 1 red: `subject_data_reports_the_readers_own_rows` |
**Two mutations of mine failed to be verdicts, and both are worth recording.**
- The first version of the export test **passed 10/10 against mutated production code.** It
  was testing and went green while the code was broken — the same mistake as
  `erase_account`, and its header states plainly that a green run is **not** evidence the
explicit `::timestamptz` cast. Three failed attempts on one test is the cost of writing it from
