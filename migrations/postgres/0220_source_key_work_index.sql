-- M45-75.8 — source-key/work index (PostgreSQL arm).
--
-- The live EXPLAIN for `source:qq OR source:sv` used
-- `idx_library_items_source_likes` for one branch but sequentially scanned
-- `library_items` for the other. The query's correlated EXISTS needs both
-- columns together; this index is additive and writes no row data.

CREATE INDEX IF NOT EXISTS idx_library_items_source_work
    ON public.library_items (source_key, work_id);
