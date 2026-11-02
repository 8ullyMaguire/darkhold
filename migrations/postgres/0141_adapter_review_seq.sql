-- 0141 — §30: `adapter_reviews` gets a monotonic `seq` (M47-10).
--
-- The reasoning is in the SQLite file: `created_at` has one-second resolution, so two reviews in
-- the same second had no order and the random `id` tiebreak was not one.
--
-- PostgreSQL numbers existing rows from a real sequence, which is atomic under concurrency where
-- the SQLite arm relies on that engine's single write lock.

ALTER TABLE adapter_reviews ADD COLUMN seq BIGINT;

CREATE SEQUENCE adapter_reviews_seq_seq;

UPDATE adapter_reviews SET seq = nextval('adapter_reviews_seq_seq') WHERE seq IS NULL;

ALTER TABLE adapter_reviews ALTER COLUMN seq SET DEFAULT nextval('adapter_reviews_seq_seq');
ALTER SEQUENCE adapter_reviews_seq_seq OWNED BY adapter_reviews.seq;

CREATE INDEX IF NOT EXISTS adapter_reviews_submission_seq
    ON adapter_reviews (submission_id, seq);
