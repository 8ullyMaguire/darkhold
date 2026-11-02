-- 0138 — `comments.created_at` becomes a shape the database keeps (M45-61).
--
-- The SQLite twin carries the shared reasoning. This file is the PostgreSQL arm, kept statement for
-- statement with it so the two can be compared line by line.
--
-- Why this table is worth a constraint at all: the welcome rota compares TEXT to TEXT
-- (`created_at >= ?`, crates/db/src/welcome.rs), so a row stored as `2026-11-02 02:13:48+00` is
-- never `>=` a bound written `2026-11-02T02:13:48Z`. PostgreSQL sorts the string; it does not parse
-- the time. The comparison matches nothing and raises nothing — the reader who commented nine
-- minutes ago is silently told they have not commented at all. A wrong answer, not a slow one, and
-- the only durable fix is for the database to refuse the shape.
--
-- Live rows are canonical: `community::insert_comment` writes `now_rfc3339`. The gap 0138 closes is
-- that nothing stopped the *next* writer. It was not hypothetical — a test fixture wrote
-- `$4::timestamptz`, PostgreSQL stored `2026-11-02 02:13:48+00`, and the window matched nothing.

-- Backfill any row that predates the rule, with the same CASE 0132 uses: a bare date is midnight,
-- an offset-bearing value is cast and formatted, a zone-less value is read as UTC (the rule M63-02
-- applies at read time).
UPDATE comments
   SET created_at = CASE
         WHEN created_at ~ '^\d{4}-\d{2}-\d{2}$'
           THEN created_at || 'T00:00:00Z'
         ELSE to_char(((CASE WHEN created_at ~ '(Z|[+-][0-9]{2}:[0-9]{2})$'
                             THEN created_at::timestamptz
                             ELSE created_at::timestamp AT TIME ZONE 'UTC' END) AT TIME ZONE 'UTC'),
                      'YYYY-MM-DD"T"HH24:MI:SS"Z"')
       END
 WHERE created_at IS NOT NULL
   AND created_at !~ '^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$';

-- NOT VALID then VALIDATE: added without scanning the table under an exclusive lock, then checked.
-- Both statements live in this migration, so a failure to validate fails the migration rather than
-- leaving an unvalidated check behind.
ALTER TABLE comments
    ADD CONSTRAINT comments_canonical_created_at CHECK (
         created_at ~ '^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$'
    ) NOT VALID;

ALTER TABLE comments VALIDATE CONSTRAINT comments_canonical_created_at;

-- Deliberately NOT constrained here: `edited_at` and `deleted_at`. They are nullable and no window
-- query reads them, so a constraint would be a rule with no reader — and the failure it would
-- prevent (a mis-shaped edit time) has no comparison to be silently wrong against. When a query
-- starts comparing them, it takes its own constraint in its own migration (docs/known-gaps.md).
