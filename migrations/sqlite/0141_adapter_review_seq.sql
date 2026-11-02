-- 0141 — §30: `adapter_reviews` gets a monotonic `seq` (M47-10).
--
-- `reviews_for` orders by `created_at, id` because a reviewer should read the reasoning in the
-- order it was written. `created_at` has ONE-SECOND resolution (M63-31) and `id` is a random UUID
-- used only as a tiebreak — so two reviews written in the same second came back in an arbitrary
-- order, and `reviews_are_returned_oldest_first` failed on exactly that: the second review led.
--
-- The same defect as `trust_events` (0140). `seq` is assigned at insert from `MAX(seq) + 1` inside
-- the statement, which SQLite's single write lock makes atomic; PostgreSQL uses a sequence.
--
-- Dialect: SQLite.

ALTER TABLE adapter_reviews ADD COLUMN seq INTEGER;

-- Existing rows are numbered in physical order, which for an append-only review table is the order
-- they were written.
UPDATE adapter_reviews SET seq = rowid WHERE seq IS NULL;

CREATE INDEX IF NOT EXISTS adapter_reviews_submission_seq
    ON adapter_reviews (submission_id, seq);
