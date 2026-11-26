# docs/plans/automatic-work-lifecycle.md — plan card

Source: `docs/plans/automatic-work-lifecycle.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

**Spec:** `docs/spec-amendments/automatic-work-lifecycle.md`
**Roadmap:** `docs/ROADMAP-automatic-work-lifecycle.md`
- Every store statement is written twice: `db.sql(sqlite, postgres)`, or a
- Targeted tests only. No `gate.sh all`, no full-workspace run mid-turn.
- `migrations/sqlite/0218_work_completion_marks.sql`
- `migrations/postgres/0218_work_completion_marks.sql`
Use the `0217_user_lists.sql` arm as the template. The `UPDATE ... SET
- `crates/domain/src/work_lifecycle.rs` (new)
- `crates/domain/src/lib.rs` (add `pub mod work_lifecycle;` beside
File: `crates/db/src/work_lifecycle.rs` (new), and `pub mod work_lifecycle;`
in `crates/db/src/lib.rs` beside `pub mod wip_adoption;`.
    let sql = db.sql(CANDIDATES_SQLITE, CANDIDATES_POSTGRES);

## Copy-pasteable code / commands

ssh thinkcentre 'cd /personal/documents/code/rust/darkhold && git status -sb'
cd ~/code-local/rust/darkhold && git status -sb

ssh thinkcentre 'cd /personal/documents/code/rust/darkhold && \
  env -u DATABASE_URL -u DARKHOLD_TEST_PG_URL \
  cargo test -p darkhold-db --test content_state --no-fail-fast'

-- 0218: automatic completion lifecycle (M45-76).
--
-- `completion_source` says who last moved completion. `import` is the honest
-- default for every row that existed before this migration: the pawchive
-- importer wrote 15,220 abandoned rows and this tree has no record of a human
-- decision for any of them.
--
-- `completion_marked_at` is the comparison point for revival. It is seeded from
-- `updated_at` for the existing abandoned rows: an import that loaded a work
-- yesterday did not see a story update after it.

ALTER TABLE works ADD COLUMN completion_source TEXT NOT NULL DEFAULT 'import'
    CHECK (completion_source IN ('import', 'owner', 'review', 'sweep', 'update'));

ALTER TABLE works ADD COLUMN completion_marked_at TEXT;

UPDATE works
   SET completion_marked_at = updated_at
 WHERE completion = 'abandoned' AND completion_marked_at IS NULL;

CREATE TABLE IF NOT EXISTS work_completion_marks (
    id                     TEXT PRIMARY KEY,
    work_id                TEXT NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    from_completion        TEXT NOT NULL,
    to_completion          TEXT NOT NULL,
    source                 TEXT NOT NULL
                           CHECK (source IN ('import', 'owner', 'review', 'sweep', 'update')),
    actor_account_id       TEXT REFERENCES accounts(id) ON DELETE SET NULL,
    note                   TEXT,
    observed_last_activity TEXT,
    marked_at              TEXT NOT NULL,
    CHECK (from_completion IN ('in_progress', 'complete', 'hiatus', 'abandoned')),
    CHECK (to_completion IN ('in_progress', 'complete', 'hiatus', 'abandoned'))
);

CREATE INDEX IF NOT EXISTS work_completion_marks_work_idx
    ON work_completion_marks (work_id, marked_at DESC);

## Verification command and expected output

- Every new test runs on SQLite **and** PostgreSQL. Set
- Targeted tests only. No `gate.sh all`, no full-workspace run mid-turn.
- Build and test on `thinkcentre`. The workstation has no Rust toolchain for
  routes, so both get an inventory row and the inventory test stays 5/5.
Expected: both clean, or only the docs commit from this task. If the deploy
  cargo test -p darkhold-db --test content_state --no-fail-fast'
Expected: the current suite is green. A red suite is noted before the first
  cargo test -p darkhold-db --test migrations --no-fail-fast'
Expected: migration applies on SQLite; the PostgreSQL arm is exercised by the
same test with `DARKHOLD_TEST_PG_URL` set, expected 0 pending and 0 failed.
#[cfg(test)]
    #[test]
    #[test]
    #[test]
    #[test]
    #[test]
    #[test]
  cargo test -p darkhold-domain work_lifecycle --no-fail-fast'
Expected: 7 passed, 0 failed. Then break the four-month test by changing
File: `crates/db/tests/work_lifecycle.rs`. Use the repo's test harness

## RED/GREEN evidence

Expected: the current suite is green. A red suite is noted before the first
same test with `DARKHOLD_TEST_PG_URL` set, expected 0 pending and 0 failed.
Expected: 7 passed, 0 failed. Then break the four-month test by changing
`months * 30` to `months * 31`; it must go red. Restore.
Expected: 7 passed on each engine. If the PostgreSQL run cannot connect, the
tunnel is not up; do not call the test passed.
            tracing::warn!(work = %work_id, %error, "failed to record owner completion mark");
Expected: 13 passed on SQLite, 13 passed on PostgreSQL, 0 failed.
- Remove `source_says_complete` from `decide`; test 4 goes red.
  `in_progress`; test 5 goes red.
- Remove `AND completion = ?5` from the update; the race test goes red.
- Remove `last > marked` in `decide`; test 6 goes red.
