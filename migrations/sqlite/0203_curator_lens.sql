-- 0203 — §0.3 curator lenses: a rec-blogger's published view of the catalogue (M45-40).
--
-- A lens is opt-in and revocable, and it belongs to the person whose taste it is: `account_id` is the primary key because a
-- person has one view, not a set of them, and publishing twice is the same view edited.
--
-- The `blurb` is required because a lens with no sentence about it is a list ordered by something the reader cannot see —
-- which is the thing §0.3 forbids for the operator's lens, and would be no better when a curator does it.
CREATE TABLE IF NOT EXISTS curator_lens (
    account_id   TEXT NOT NULL PRIMARY KEY,
    blurb        TEXT NOT NULL,
    published_at TEXT NOT NULL,
    updated_at   TEXT NOT NULL
);
