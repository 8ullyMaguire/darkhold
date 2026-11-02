-- 0140 — §19.4: trust_events gets a monotonic `seq` (M47-10).
--
-- The reasoning is in the SQLite file: `created_at` has one-second resolution, so two events in
-- the same second had no order and `id` (a random UUID) is not a tiebreak.
--
-- PostgreSQL gets a real sequence rather than an application-computed `MAX + 1`, because it can:
-- `nextval` is atomic under concurrency, where the SQLite arm relies on that engine's single write
-- lock. Existing rows are numbered in physical order, which for an append-only table is insertion
-- order.
--
-- The append-only trigger is dropped and recreated around the backfill, because the backfill is an
-- UPDATE and the trigger exists to refuse those.

ALTER TABLE trust_events ADD COLUMN seq BIGINT;

CREATE SEQUENCE trust_events_seq_seq;

DROP TRIGGER trust_events_refuse_update ON trust_events;
UPDATE trust_events SET seq = nextval('trust_events_seq_seq') WHERE seq IS NULL;
CREATE TRIGGER trust_events_refuse_update
BEFORE UPDATE ON trust_events
FOR EACH ROW EXECUTE FUNCTION trust_events_refuse_change();

ALTER TABLE trust_events ALTER COLUMN seq SET DEFAULT nextval('trust_events_seq_seq');
ALTER SEQUENCE trust_events_seq_seq OWNED BY trust_events.seq;

CREATE INDEX IF NOT EXISTS trust_events_account_seq ON trust_events (account_id, seq DESC);
