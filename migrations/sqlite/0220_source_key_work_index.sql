-- M45-75.8 — source-key/work index (SQLite arm).
--
-- Mirrors the PostgreSQL arm exactly. Additive DDL: no row is rewritten, and
-- `IF NOT EXISTS` keeps a restart after a partial upgrade harmless.

CREATE INDEX IF NOT EXISTS idx_library_items_source_work
    ON library_items (source_key, work_id);
