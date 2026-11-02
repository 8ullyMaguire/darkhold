-- 0139 — §9.6 private search history (audit item 31, M47-08).
--
-- A reader's last `N` DISTINCT searches, per pseud. Three decisions, each with a way it could
-- have gone wrong:
--
-- `query_key` is a COLUMN rather than an expression index on `lower(query)`. SQLite's `lower()`
-- folds ASCII only, so on that engine `Éowyn` and `éowyn` would be two rows while PostgreSQL
-- made them one -- the two engines would disagree about how long a reader's history is. The
-- normalisation (trim, collapse whitespace, `to_lowercase`) happens once in Rust and both engines
-- then compare identical bytes.
--
-- `pseud_id` is TEXT with NO foreign key, following 0137: pseud ids are UUID on PostgreSQL and
-- TEXT in the recent tables, and §51 erases an account by REKEYING its pseud rather than deleting
-- it, so a cascade would never fire. There is deliberately no `account_id` column: the history is
-- keyed on the pseud, and an account column would make a rekeyed pseud's history reachable by the
-- account that used to own it.
--
-- `result_count` is stored, not recomputed. It is a fact about that search -- running the same
-- query tomorrow returns a different number, and a history that quietly disagreed with what the
-- reader saw would be worse than one showing no number.
--
-- Timestamps are canonical `YYYY-MM-DDTHH:MM:SSZ` (M63-31, migration 0132).

CREATE TABLE IF NOT EXISTS search_history (
    id           TEXT NOT NULL PRIMARY KEY,
    pseud_id     TEXT NOT NULL,
    query        TEXT NOT NULL,
    query_key    TEXT NOT NULL,
    -- BIGINT, not INTEGER: the store reads this as `i64`, and sqlx refuses an INT4 column
    -- decoded into INT8 with "mismatched types; Rust type `i64` (as SQL type `INT8`) is not
    -- compatible with SQL type `INT4`". SQLite's INTEGER is 64-bit already, so the two dialects
    -- agree only if PostgreSQL says BIGINT.
    result_count BIGINT NOT NULL,
    searched_at  TEXT NOT NULL
);

-- One row per distinct query per pseud: re-running a query moves it to the front instead of
-- appearing twice, which is what makes this a history rather than a log.
CREATE UNIQUE INDEX IF NOT EXISTS search_history_pseud_key
    ON search_history (pseud_id, query_key);

CREATE INDEX IF NOT EXISTS search_history_pseud_recent
    ON search_history (pseud_id, searched_at DESC);
