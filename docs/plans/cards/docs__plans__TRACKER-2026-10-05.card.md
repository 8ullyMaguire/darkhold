# docs/plans/TRACKER-2026-10-05.md — plan card

Source: `docs/plans/TRACKER-2026-10-05.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

Canonical inventory is **`docs/requirements.csv`** (703 rows). This file records what
3. **The migration parser merged two definitions of `mutes`.** `0001_identity.sql` creates
   it `(id, pseud_id, target_type, target_id, …)`; `0013_community.sql` drops and
`reader_surface_t1.rs` and `continue_reading_t1.rs` are **byte-identical from line 60 to
`crates/db/src/erasure.rs` carried **10 `cargo fmt` diffs** at HEAD, last touched by
**Before any next session runs the PG leg:** `scripts/pg-test-db.sh`, then export the URL

## Copy-pasteable code / commands

No fenced command block was recorded in the source document.

## Verification command and expected output

# Darkhold — tracker, 2026-10-05 (close of session, second pass)
| Gate | Tree | Result |
| Rust, SQLite workspace | `76c2cc6` | **4160 passed / 0 failed / 210 suites** (exit 0) |
| `cargo clippy --workspace --all-targets` | `76c2cc6` | exit 0, **0 warnings** (denied in this repo) |
| `test-support --lib` | — | **6/6 both engines** |
`snapshot_anonymisation` 8, `test-support --lib` 1, plus 1 more surfaced by the clean
Frontend figures are from the earlier pass and were **not** re-measured today:
   could only pass on a host serving PostgreSQL on the default port. All now route
   these test schemas are not, so 8 parallel migrators reach 0092 together and all
Verified at **8 threads**, the level that reproduced both, twice. `--test-threads=1`
**failing on committed code** the whole time. A gate that passes because it is not looking
| **PG + SQLite workspace at `651cc9b`** | **in flight** — this is the one gate that closes the session |
| Frontend re-measurement | vitest / svelte-check / Playwright figures above are from the earlier pass, not re-run against `651cc9b` |
**Before any next session runs the PG leg:** `scripts/pg-test-db.sh`, then export the URL

## RED/GREEN evidence

| Rust, PostgreSQL workspace | `76c2cc6` | **4126 passed / 3 failed / 210 suites** — the 3 since fixed in `651cc9b` |
| Rust, SQLite workspace | `76c2cc6` | **4160 passed / 0 failed / 210 suites** (exit 0) |
The suite had **never been green on any engine**. Each fault was invisible until the
   `pg_dump` reported `password authentication failed for user "postgres://darkhold"` —
Plus the restore/`doctor` wiring: `pg_scoped_url("restored")` passed a **literal** where
