-- 0139 — §9.6 private search history (audit item 31, M47-08).
--
-- Dialect: SQLite. Columns mirror the PostgreSQL file exactly in name and order; the
-- migration-catalogue test asserts that, and types may differ by dialect.
--
-- The reasoning (why `query_key` is a column, why there is no foreign key and no `account_id`,
-- why `result_count` is stored) is in the PostgreSQL file and is not repeated here.

CREATE TABLE IF NOT EXISTS search_history (
    id           TEXT NOT NULL PRIMARY KEY,
    pseud_id     TEXT NOT NULL,
    query        TEXT NOT NULL,
    query_key    TEXT NOT NULL,
    result_count INTEGER NOT NULL,
    searched_at  TEXT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS search_history_pseud_key
    ON search_history (pseud_id, query_key);

CREATE INDEX IF NOT EXISTS search_history_pseud_recent
    ON search_history (pseud_id, searched_at DESC);
