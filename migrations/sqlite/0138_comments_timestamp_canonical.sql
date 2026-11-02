-- 0138 — `comments.created_at` becomes a shape the database keeps (M45-61).
--
-- The defect, in one line: the welcome rota compares TEXT to TEXT
-- (`created_at >= ?`, crates/db/src/welcome.rs:426). A row stored as
-- `2026-11-02 02:13:48+00` is never `>=` a bound written `2026-11-02T02:13:48Z`, because the
-- comparison is a string comparison and a space is not a `T`. The window then matches nothing and
-- returns no error: the reader who commented nine minutes ago is told they have not commented at
-- all. Nothing logs it. The only durable fix is for the database to refuse the shape, because the
-- next writer is by definition the one that has not read this comment.
--
-- Live rows are canonical — `community::insert_comment` writes `now_rfc3339` — so this migration
-- backfills first and constrains second. It was not hypothetical: a test fixture wrote
-- `$4::timestamptz`, PostgreSQL stored the space-and-offset form, and the window matched nothing.
--
-- SQLite cannot add a CHECK to an existing table without rebuilding it, so the same rule is a pair
-- of triggers per column. `GLOB` rather than `LIKE`: `LIKE` has no character classes, and `GLOB`
-- lets the trigger reject the row without calling a function on it first. Statement for statement
-- with the PostgreSQL twin.

-- `strftime('%Y-%m-%dT%H:%M:%SZ', x)` reads the offset-bearing and zone-less shapes and normalises
-- to UTC (SQLite's date functions treat a zone-less value as UTC, the same rule M63-02 applies at
-- read time), so the backfill is one expression rather than a CASE over shapes.
UPDATE comments
   SET created_at = strftime('%Y-%m-%dT%H:%M:%SZ', created_at)
 WHERE created_at IS NOT NULL
   AND created_at NOT GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T[0-9][0-9]:[0-9][0-9]:[0-9][0-9]Z';

CREATE TRIGGER comments_canonical_created_at_insert
BEFORE INSERT ON comments
WHEN NEW.created_at IS NOT NULL
 AND NEW.created_at NOT GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T[0-9][0-9]:[0-9][0-9]:[0-9][0-9]Z'
BEGIN
    SELECT RAISE(ABORT, 'created_at must be canonical UTC (…Z)');
END;

CREATE TRIGGER comments_canonical_created_at_update
BEFORE UPDATE OF created_at ON comments
WHEN NEW.created_at IS NOT NULL
 AND NEW.created_at NOT GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T[0-9][0-9]:[0-9][0-9]:[0-9][0-9]Z'
BEGIN
    SELECT RAISE(ABORT, 'created_at must be canonical UTC (…Z)');
END;

-- Deliberately NOT constrained here: `edited_at` and `deleted_at`. They are nullable and no window
-- query reads them, so a constraint would be a rule with no reader — and the failure it would
-- prevent has no comparison to be silently wrong against. When a query starts comparing them, it
-- takes its own constraint in its own migration (docs/known-gaps.md).
