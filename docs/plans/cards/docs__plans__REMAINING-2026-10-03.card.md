# docs/plans/REMAINING-2026-10-03.md — plan card

Source: `docs/plans/REMAINING-2026-10-03.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

| PostgreSQL | **`scripts/pg-test-db.sh`** — run it; it creates/starts `lh-pg-test` and prints the URL. Container on **127.0.0.1:55433**, user `darkhold`, db `postgres`, password `darkhold`. **It must be created with `--shm-size 4g`** (see `pg-test-db.sh`): Docker's 64 MB default produces `Connection reset by peer`, `out of shared memory (53200)` and `could not resize shared memory segment` — all one fault — and `ShmSize` is fixed at create time, so `docker start` cannot repair it. |
| 1 | M45-57 HTTP routes + §55.5 check | 7–8 | `source_adapter_routes.rs` 9/9, `curator.rs` 5/5, `darkhold-scrapers` 364/364 |
| 3 | M45-22 domain + store | 3–4 | `concierge_store.rs` 13/13 both engines |
| 4 | M45-22 routes + WIP notify | 5–6 | `concierge_routes.rs` 11/11 **both engines** |
   `concierge_uuid_binds.rs` covers both call sites.
| 1 | `spec.md` §54 → implementation note | **Done.** §54 carries the note (shipped `baf297d`, tag `v0.56-concierge`, five routes at `crates/app/src/routes/concierge.rs`), and the grep it promises works: `grep -rn concierge crates/app/src/routes/mod.rs` → `pub mod concierge;` on line 18. The row stayed open because the note was written *after* the list; verified `2026-11-02` and closed. |
| 2b | `check-uncast-pg-placeholders` | **Closed — and it found two real defects on the way, so the fix was NOT "make the gate quiet".** Measured on PostgreSQL 15.19, not reasoned about. **15 sites, 0 real.** Three rules were firing on correct code, and the semantics are now pinned in the file's self-test (34/34): **(a)** a uuid or NUMERIC placeholder needs no cast — PostgreSQL infers an untyped parameter from the column, proven with `PREPARE` against both a uuid and a text column; what actually fails is binding a **`&str`** to a uuid column (`incorrect binary data format`), a different fault the sites do not have because they bind a parsed `Uuid`. **(b)** `COALESCE(SUM(amount_bp), 0)` over NUMERIC decodes fine — `retention_proposals` 12/12, `preservation_ledger` 7/7, `payout_recalc_wiring` 12/12 on PG. **(c)** **assignment coerces, comparison does not** — `UPDATE t SET ts = $1::timestamptz` into a TEXT column returns UPDATE 1, `WHERE ts <= $1::timestamptz` errors with `operator does not exist`. Rule (c) was rewritten to comparison-only; (a) and (b) are left in place because deleting a rule that has caught real bugs needs a replacement that catches the same bug. **The two defects it DID find are fixed and committed:** `hit_rate.rs` (missing `FROM` alias — the gate named the file, not the fault) and `tasting.rs:1082` (a true INT4→i64 decode, which **no test could reach** — see row 2c). |
| 2c | `tasting.rs` INT4→i64 decode | **Closed.** `existing_match_state` decoded `arena_weights.matches_played` (INTEGER → INT8 on SQLite, INT4 on PostgreSQL) as `i64`, which fails on PostgreSQL with `mismatched types; Rust type i64 (as SQL type INT8) is not compatible with SQL type INT4`. Fixed with `CAST(... AS BIGINT)` — `::bigint` is PostgreSQL-only and the statement is spelled once for both backends. **No test caught it and none could**: the statement is reached from `POST /api/v1/tasting/respond` only once a response moves a weight, and an `if (next - current).abs() < f64::EPSILON { return Ok(false) }` guard above the call means every test that got that far returned early — `arena` 4/4 and `tasting_menu` 14/14 both pass on PostgreSQL *with the bug in place*. Found by reading plus a direct probe. `crates/db/tests/arena_weights_decode.rs` is the regression, and its load-bearing assertion is that **`src/tasting.rs` still spells the cast** — a hand-copied statement in a test drifts from the original, and the first version of that file passed happily while the real query had already been reverted. Verified by reverting: green, then red. |
| 3 | The `planned` tracker rows | **Counted, not estimated.** `python3 scripts/backlog.py` writes `docs/backlog/INVENTORY.md`, and `--check` fails when it is stale. As of `2026-11-02`: **79** open (`M63` 32, `M45` 23, `M64` 14, `M66` 6, `M65` 4), of which one closed here (`M66-09`). The earlier "27 + 6" was the `2026-10-03` count and had drifted by 46 rows, which is why the number is generated now. |
| 4 | Path B (WASM), §55.4 | Gated by §55.6. `scripts/check-wasm-gate.py` fails the build if a WASM runtime is adopted; `wasmi 0.4` does not compile on this toolchain. |
| 5 | The split spec covers §1–27 only | **Closed, 2026-11-02.** §28–61 are now slots in the split as well, and the gate reads both masters. The note said §28–61 "live in `docs/spec.md`"; measured, they are written in **two heading shapes** — `# N. Title` for §28–49 and `## N Title` (no dot) for §50–61 — which is why a parser that looked for one shape reported the document as having no sections past 27. `scripts/split-spec.py` now reads the vault file for §1–27 and `docs/spec.md` for §28–61, the numbers stay contiguous, the reserved block moved to 62–70, and the index states which master governs which range. **Proof the gate bites:** changing §53.5 in `docs/spec.md` now fails `check-split-spec.py` with `53-faucets-and-sinks-the-economy-as-an-operator.md differs from its master`; before this change the same edit was invisible. A stale `NN-reserved.md` that a section replaced is deleted by the writer and reported by the checker, because two files claiming slot 28 is the same trap one level down. |
| 6 | `docs/backlog/` is generated and gated | **Done.** `scripts/backlog.py` joins the tracker, the Concord snapshot and this file; `--check` fails on staleness. Its first matcher reported *zero* unlinked Concord cards because an empty `requirement` cell makes the needle empty and `"" in anything` is true — the same "0 grep is not a 0" trap `spec-gap-analysis` records. |

## Copy-pasteable code / commands

panicked at crates/test-support/src/lib.rs: connect: connecting to SQLite at ...
Caused by: pool timed out while waiting for an open connection

Rust type `core::option::Option<i32>` (as SQL type `INT4`) is not compatible with
SQL type `INT8`

because the only reason it was caught is that the full-workspace run was happening anyway,
which is the argument for keeping doing it.

---

## Sixth pass — the same defect twice, and a gate that could not see its own input

Two PostgreSQL defects, found by a working gate, plus two silent gates that had been
reporting nothing while looking healthy.

### `analytics.rs` — a masked 500 on every reader's own reading totals

`READING_TOTALS_POSTGRES` had two unaliased subqueries in `FROM`:


## Verification command and expected output

| PostgreSQL | **`scripts/pg-test-db.sh`** — run it; it creates/starts `lh-pg-test` and prints the URL. Container on **127.0.0.1:55433**, user `darkhold`, db `postgres`, password `darkhold`. **It must be created with `--shm-size 4g`** (see `pg-test-db.sh`): Docker's 64 MB default produces `Connection reset by peer`, `out of shared memory (53200)` and `could not resize shared memory segment` — all one fault — and `ShmSize` is fixed at create time, so `docker start` cannot repair it. |
| Test URL | `export DARKHOLD_TEST_PG_URL='postgres://darkhold:darkhold@127.0.0.1:55433/postgres'` — **both engines means running the suite twice**, once with this set and once without. `unset` it before the SQLite run; a leftover value sends every test to a host that does not resolve. |
| Frontend | `cd frontend && node node_modules/vitest/vitest.mjs run <file>`. Not `pnpm test`. |
| Mutation runs | One `cargo test` on the app suite is ~10 s SQLite, ~190 s PostgreSQL. Seven mutations do not fit a 300 s cell — run them backgrounded via a script, not inline in `execute_code`. |
| 5 | M45-22 frontend | 7 | `Concierge.test.ts` 11/11 |
| 2b | `check-uncast-pg-placeholders` | **Closed — and it found two real defects on the way, so the fix was NOT "make the gate quiet".** Measured on PostgreSQL 15.19, not reasoned about. **15 sites, 0 real.** Three rules were firing on correct code, and the semantics are now pinned in the file's self-test (34/34): **(a)** a uuid or NUMERIC placeholder needs no cast — PostgreSQL infers an untyped parameter from the column, proven with `PREPARE` against both a uuid and a text column; what actually fails is binding a **`&str`** to a uuid column (`incorrect binary data format`), a different fault the sites do not have because they bind a parsed `Uuid`. **(b)** `COALESCE(SUM(amount_bp), 0)` over NUMERIC decodes fine — `retention_proposals` 12/12, `preservation_ledger` 7/7, `payout_recalc_wiring` 12/12 on PG. **(c)** **assignment coerces, comparison does not** — `UPDATE t SET ts = $1::timestamptz` into a TEXT column returns UPDATE 1, `WHERE ts <= $1::timestamptz` errors with `operator does not exist`. Rule (c) was rewritten to comparison-only; (a) and (b) are left in place because deleting a rule that has caught real bugs needs a replacement that catches the same bug. **The two defects it DID find are fixed and committed:** `hit_rate.rs` (missing `FROM` alias — the gate named the file, not the fault) and `tasting.rs:1082` (a true INT4→i64 decode, which **no test could reach** — see row 2c). |
| 2c | `tasting.rs` INT4→i64 decode | **Closed.** `existing_match_state` decoded `arena_weights.matches_played` (INTEGER → INT8 on SQLite, INT4 on PostgreSQL) as `i64`, which fails on PostgreSQL with `mismatched types; Rust type i64 (as SQL type INT8) is not compatible with SQL type INT4`. Fixed with `CAST(... AS BIGINT)` — `::bigint` is PostgreSQL-only and the statement is spelled once for both backends. **No test caught it and none could**: the statement is reached from `POST /api/v1/tasting/respond` only once a response moves a weight, and an `if (next - current).abs() < f64::EPSILON { return Ok(false) }` guard above the call means every test that got that far returned early — `arena` 4/4 and `tasting_menu` 14/14 both pass on PostgreSQL *with the bug in place*. Found by reading plus a direct probe. `crates/db/tests/arena_weights_decode.rs` is the regression, and its load-bearing assertion is that **`src/tasting.rs` still spells the cast** — a hand-copied statement in a test drifts from the original, and the first version of that file passed happily while the real query had already been reverted. Verified by reverting: green, then red. |
| 4 | Path B (WASM), §55.4 | Gated by §55.6. `scripts/check-wasm-gate.py` fails the build if a WASM runtime is adopted; `wasmi 0.4` does not compile on this toolchain. |
| 5 | The split spec covers §1–27 only | **Closed, 2026-11-02.** §28–61 are now slots in the split as well, and the gate reads both masters. The note said §28–61 "live in `docs/spec.md`"; measured, they are written in **two heading shapes** — `# N. Title` for §28–49 and `## N Title` (no dot) for §50–61 — which is why a parser that looked for one shape reported the document as having no sections past 27. `scripts/split-spec.py` now reads the vault file for §1–27 and `docs/spec.md` for §28–61, the numbers stay contiguous, the reserved block moved to 62–70, and the index states which master governs which range. **Proof the gate bites:** changing §53.5 in `docs/spec.md` now fails `check-split-spec.py` with `53-faucets-and-sinks-the-economy-as-an-operator.md differs from its master`; before this change the same edit was invisible. A stale `NN-reserved.md` that a section replaced is deleted by the writer and reported by the checker, because two files claiming slot 28 is the same trap one level down. |
The 2026-10-03 evening pass ran every gate this project has. Two of them had **never
| Gate | Before today | Result |
| `cargo test --workspace` | **never run** | **12–17 failures** → 4052/4052 after two fixes |
panicked at crates/test-support/src/lib.rs: connect: connecting to SQLite at ...
Every affected suite passes 100% alone — at `--test-threads` 16, 4 and 1, and two of
them concurrently also pass. So it was never a defect in any suite, and calling it
**Cause.** `cargo test --workspace` runs 198 suites; each `TestDb` clones a migrated
file and opens its own `SqlitePool`. 134 of the 182 test files never call
1. `test_db_config()` widens the *test* acquire timeout to 60s. Production defaults
   Plus `sweep_stale_sqlite()` for the databases a failing test never returns from.
`pg_database` about attached backends. It is deliberately **not** wired into the test

## RED/GREEN evidence

| 6 | `route_inventory` working | — | 3/3, first time green in the file's history |
   `bind_work_ids!`; proved green on SQLite and red on PostgreSQL against a reverted
   removing either leaves the suite green (measured). Both were proved by corrupting
| 2c | `tasting.rs` INT4→i64 decode | **Closed.** `existing_match_state` decoded `arena_weights.matches_played` (INTEGER → INT8 on SQLite, INT4 on PostgreSQL) as `i64`, which fails on PostgreSQL with `mismatched types; Rust type i64 (as SQL type INT8) is not compatible with SQL type INT4`. Fixed with `CAST(... AS BIGINT)` — `::bigint` is PostgreSQL-only and the statement is spelled once for both backends. **No test caught it and none could**: the statement is reached from `POST /api/v1/tasting/respond` only once a response moves a weight, and an `if (next - current).abs() < f64::EPSILON { return Ok(false) }` guard above the call means every test that got that far returned early — `arena` 4/4 and `tasting_menu` 14/14 both pass on PostgreSQL *with the bug in place*. Found by reading plus a direct probe. `crates/db/tests/arena_weights_decode.rs` is the regression, and its load-bearing assertion is that **`src/tasting.rs` still spells the cast** — a hand-copied statement in a test drifts from the original, and the first version of that file passed happily while the real query had already been reverted. Verified by reverting: green, then red. |
| Playwright E2E | run once | 84/84 → re-run green |
| 8 static CI gates | 6 red | 5 closed, 1 non-failure; 2 gates still red on untouched files |
   4: four previously-failing suites run concurrently passed 81/81 **both before and
131. The leak fix is what closed it — the final run is 198 suites / **4052 passed /
0 failed / 0 pool timeouts**, at default parallelism, exit 0.
Built, with six tests. Three exist because a mutation came back GREEN, and the third is
| moods not parsed out of the message | 2 failed — CAUGHT |
| a 500 swallowed into the mood path | 1 failed — CAUGHT |
