# docs/plans/m45-61-comments-timestamp-canonical.md — plan card

Source: `docs/plans/m45-61-comments-timestamp-canonical.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

**Requirement row:** `M45-61` (`docs/requirements.csv`) · **Milestone:** M45 · **Status:** planned →
… AND created_at >= ?          -- crates/db/src/welcome.rs:426
`M63-31` put exactly this constraint on `library_items` (`migrations/*/0132`). This applies the same
### 1. `migrations/postgres/0138_comments_timestamp_canonical.sql`
### 2. `migrations/sqlite/0138_comments_timestamp_canonical.sql`
### 3. `crates/app/tests/comments_timestamp_canonical.rs`
* `docs/requirements.csv` `M45-61` → `implemented-fully-tested`, with the evidence and the note that
* `docs/verification.md` entry.
* `docs/known-gaps.md`: `comments.edited_at` and `deleted_at` are nullable and are **not** covered by
python3 scripts/backlog.py --check                                     # ledger current

## Copy-pasteable code / commands

… AND created_at >= ?          -- crates/db/src/welcome.rs:426

cargo test -p darkhold-app --test comments_timestamp_canonical        # SQLite: 4 passed
LOREHAVEN_TEST_PG_URL=… cargo test -p darkhold-app --test comments_timestamp_canonical   # PostgreSQL: 4 passed
python3 scripts/backlog.py --check                                     # ledger current

## Verification command and expected output

nothing stops the next writer.** A test fixture already proved it: it wrote `$4::timestamptz`,
| test | what it proves |
cargo test -p darkhold-app --test comments_timestamp_canonical        # SQLite: 4 passed
LOREHAVEN_TEST_PG_URL=… cargo test -p darkhold-app --test comments_timestamp_canonical   # PostgreSQL: 4 passed
Expected on the unfixed tree: test 1 fails (the insert succeeds), test 4 fails (the row is accepted,
the window matches nothing). Expected after: all pass. **Both proven by reverting the migration file
and re-running** — a test that passes on unfixed code is not evidence.

## RED/GREEN evidence

cargo test -p darkhold-app --test comments_timestamp_canonical        # SQLite: 4 passed
LOREHAVEN_TEST_PG_URL=… cargo test -p darkhold-app --test comments_timestamp_canonical   # PostgreSQL: 4 passed
