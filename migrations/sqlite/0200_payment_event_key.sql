-- Migration 0200 — a payment event is recorded once (§20.9.5).
--
-- Dialect: sqlite.
--
-- `payment_events` recorded the money that moves through a processor: gross, the processor's fee, and net. It had no
-- idempotency key, which is why no money path could honestly write to it: a retried tip posts one earnings row (the ledger
-- has a key) and would have posted two payment events. The key makes the write repeatable, so §20.9.5's "every monetary
-- transaction records the fee" can be true rather than aspirational.
--
-- Nullable and unique: existing rows predate the key, and a partially-unique index on a NULL-able column is exactly what
-- SQLite and PostgreSQL both do not want — so the column defaults to NULL for history and every new write supplies one.

ALTER TABLE payment_events ADD COLUMN event_key TEXT;
CREATE UNIQUE INDEX IF NOT EXISTS payment_events_event_key ON payment_events (event_key);
