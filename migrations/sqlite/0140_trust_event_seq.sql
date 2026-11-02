-- 0140 — §19.4: trust_events gets a monotonic `seq` (M47-10).
--
-- The log already ordered by `created_at DESC, id DESC`, and `created_at` has ONE-SECOND
-- resolution (M63-31's canonical stamp). Two events written for the same account in the same
-- second therefore had no defined order: `id` is a random UUID, so "newest first" was a claim the
-- storage could not keep, and `every_trust_change_writes_a_row` failed the moment the two changes
-- landed inside one second — which is what a scripted grant does.
--
-- A log needs a tiebreak that is monotonic, not random. `seq` is it: assigned at insert from
-- `MAX(seq) + 1` in the same statement, so SQLite's single write lock makes it atomic.
--
-- The append-only trigger is dropped and recreated around the backfill, because the backfill IS an
-- UPDATE and the trigger exists precisely to refuse those. Recreating it immediately afterwards is
-- the point: the column is added once, by the migration, and never rewritten after.
--
-- Dialect: SQLite. The PostgreSQL file uses a sequence, which that engine can assign atomically.

ALTER TABLE trust_events ADD COLUMN seq INTEGER;

DROP TRIGGER trust_events_refuse_update;
UPDATE trust_events SET seq = rowid WHERE seq IS NULL;
CREATE TRIGGER trust_events_refuse_update
BEFORE UPDATE ON trust_events
BEGIN
    SELECT RAISE(ABORT, 'trust_events is append-only');
END;

-- "This account's events, newest first" is now answerable without a tiebreak.
CREATE INDEX IF NOT EXISTS trust_events_account_seq ON trust_events (account_id, seq DESC);
