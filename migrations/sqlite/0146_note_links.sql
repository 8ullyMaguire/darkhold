-- 0146 — author-note links are monitored like any other link (M45-38, §32.7, gaps review D3).
--
-- A note is prose, not a table of links, so the links have to be extracted at write time: by the time
-- a note is read nobody is going to re-scan every revision on every request. This table is that
-- extraction, keyed to the revision it came from so that editing a note re-derives its links instead
-- of accumulating them.
--
-- Two decisions worth stating in the schema rather than only in code:
--
-- - **`revision_id` is the owner, not `work_id`.** A link belongs to the sentence that pointed at it;
--   a new revision with a corrected URL is a new claim, and the old one stops being checked when the
--   revision it came from is no longer the live one. `work_id` and `unit_id` are carried anyway
--   because every read is by work or by unit, and joining back to `revisions` to answer "does this
--   work have any dead links" would be a scan of the whole history for a question about now.
-- - **`status` is the existing vocabulary**, not a new one: healthy / degraded / dead / quarantined /
--   pending_verification. A note link that is "dead" means exactly what an availability link that is
--   dead means, and the checker that updates it is the same checker.
--
-- No foreign keys, for the §58.3 reason: erasure rekeys an account rather than deleting it, and
-- `revisions` rows are not deleted either — but a link row outliving the note it came from is the
-- correct state for evidence, and a cascade would quietly delete the record of what the note said.
--
-- Dialect: SQLite.

CREATE TABLE IF NOT EXISTS note_links (
    id                    TEXT    PRIMARY KEY,
    work_id               TEXT    NOT NULL,
    unit_id               TEXT    NOT NULL,
    revision_id           TEXT    NOT NULL,
    url                   TEXT    NOT NULL,
    host                  TEXT    NOT NULL,
    provider              TEXT    NOT NULL DEFAULT 'other',
    status                TEXT    NOT NULL DEFAULT 'pending_verification',
    consecutive_failures  INTEGER NOT NULL DEFAULT 0,
    last_checked_at       TEXT,
    last_healthy_at       TEXT,
    failure_reason        TEXT,
    created_at            TEXT    NOT NULL,
    updated_at            TEXT    NOT NULL,
    -- A note that says the same URL twice registers it once: an edit that only moves a paragraph must
    -- not double the work the checker has to do.
    UNIQUE (revision_id, url)
);

CREATE INDEX IF NOT EXISTS idx_note_links_work ON note_links (work_id, status);
CREATE INDEX IF NOT EXISTS idx_note_links_unit ON note_links (unit_id);
CREATE INDEX IF NOT EXISTS idx_note_links_revision ON note_links (revision_id);
-- The checker's query: oldest check first, and only rows that are not already known dead (§32.7 keeps
-- checking a dead link, at a slower cadence, because sites come back).
CREATE INDEX IF NOT EXISTS idx_note_links_check ON note_links (last_checked_at, status);
