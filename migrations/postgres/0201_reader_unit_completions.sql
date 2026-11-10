-- 0201 — the per-**completion** fact §41.2's ReadUnit needs.
--
-- `reading_unit_read` already exists and is written on the first *open* of a unit, keyed on the **pseud** — it is what the `chapters`
-- and `words` reading goals count. That is a different fact from the one warmth needs, and this table is not a duplicate of it:
--
--   reading_unit_read   : pseud, first opened, purpose = goals
--   reader_unit_completions : account, first reached the end, purpose = warmth
--
-- The scope differs because §41.2's score is between accounts, and the fact differs because §41.2's act is *reading* a unit, not
-- opening it: crediting warmth on every save would pay the reader for scrolling, and crediting on first open would make 100 bp cost one
-- click. One row per (account, unit), never updated — `first_completed_at` carries the moment the act was first seen, and an update
-- would make the column say "last" while the name promises "first".
CREATE TABLE IF NOT EXISTS reader_unit_completions (
    account_id         UUID NOT NULL,
    unit_id            UUID NOT NULL,
    work_id            UUID NOT NULL,
    first_completed_at TEXT NOT NULL,
    PRIMARY KEY (account_id, unit_id)
);
CREATE INDEX IF NOT EXISTS idx_reader_unit_completions_work ON reader_unit_completions (work_id);
