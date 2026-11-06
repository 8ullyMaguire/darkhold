-- The catalogue search's hot path is `title ILIKE '%term%' OR summary ILIKE '%term%'`, and a leading-wildcard
-- pattern cannot use a btree index. On 58,042 works that is a sequential scan reading 4,916 pages: measured 500 ms
-- warm, and it exceeded a client's timeout under a gate suite's concurrent load — which is how it surfaced, as
-- `polaris search harry` returned nothing in check-cli while answering in 0.6 s when run alone.
--
-- pg_trgm is a *trusted* extension from PostgreSQL 13 on, so the database owner (`darkhold`) can install it from a
-- migration: no superuser step in the deploy, and no restore that silently lacks the operator class.
create extension if not exists pg_trgm;

-- Partial on `deleted_at is null` because every reader filters withdrawn works out of the catalogue, and the
-- predicate is what lets the planner use the index on the common path instead of rechecking every candidate row.
create index if not exists works_title_trgm on works using gin (title gin_trgm_ops) where deleted_at is null;
create index if not exists works_summary_trgm on works using gin (summary gin_trgm_ops) where deleted_at is null;

-- Measured on the same 58,042-row catalogue, same statement, same LIMIT:
--
--   before: Seq Scan, 4,916 buffers, 500.09 ms
--   after:  BitmapOr of works_title_trgm and works_summary_trgm, 1,970 buffers, 56.29 ms
--
-- 56 ms is the *worst* term in the catalogue ('harry' matches 5,038 of 58,042 rows, 8.7%); a rare term resolves in
-- single-digit milliseconds because the index returns a handful of rows rather than a fifth of the table. The index
-- build itself is a plain CREATE INDEX rather than CONCURRENTLY because sqlx runs migrations in a transaction; on
-- 58k rows that is seconds of write lock during a deploy, not a maintenance-window problem.
