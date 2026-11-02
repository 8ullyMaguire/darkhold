-- 0144 — §9.7.6 badges, beginning with the debut badge (audit item 75, M47-14).
--
-- One row per badge per ACCOUNT, and that uniqueness is the whole mechanism: §9.7.6's milestone
-- badges are "one-time, permanent", so "award it again" is a no-op rather than a second payout.
-- The award carries the credit bonus it paid, because the amount is a fact about what happened
-- rather than a number to re-read from a constant that may have moved since.
--
-- The badge is on the ACCOUNT and the context names what earned it (the work, the pseud). A reader
-- who publishes their debut under one pseud and later adds another still has one First Words.
--
-- No `show_badges` column yet. §9.7.6 says badges appear "if user chooses to display them", and
-- this build has no per-pseud preference surface to hold that choice; the profile's own
-- `discoverability` setting already hides the page and everything on it. Recorded as the open half
-- of item 75 rather than half-built here: a boolean nothing can set is a control with no reader.
--
-- "Debut" is DERIVED, not stored. A stored `is_new_author` flag is a fact that goes stale the
-- moment a second work is published; `badges::published_work_count` answers it from the catalogue
-- every time it is asked.

CREATE TABLE IF NOT EXISTS badge_award (
    id         TEXT NOT NULL PRIMARY KEY,
    account_id TEXT NOT NULL,
    badge      TEXT NOT NULL,
    context    TEXT,
    credits    INTEGER NOT NULL DEFAULT 0,
    awarded_at TEXT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS badge_award_once
    ON badge_award (account_id, badge);

CREATE INDEX IF NOT EXISTS badge_award_badge
    ON badge_award (badge, awarded_at);
