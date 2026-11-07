-- M69-04: a tombstone keyed by WORK IDENTITY, so a takedown outlives the URL it was filed against.
--
-- §5.5 and §13.2 step 3. `dmca_takedowns` is paperwork about one local mirror; this is the thing that stops the same work
-- coming back through the importer tomorrow from another host. The key is derived from title + author + language
-- (`darkhold_domain::work_identity`), never from a URL: the author's complaint is about their story.
--
-- A withdrawal sets `withdrawn_at` rather than deleting the row. An over-broad takedown must be undoable, and the fact that
-- it once refused three imports is the evidence an operator needs when the author asks why their work disappeared.

CREATE TABLE work_tombstones (
    id              TEXT PRIMARY KEY,
    identity        TEXT NOT NULL UNIQUE,   -- work_identity::key(title, author, language)
    title           TEXT NOT NULL,          -- as filed, for a human reading the list
    author          TEXT NOT NULL,
    language        TEXT,
    reason          TEXT NOT NULL,          -- why: 'author request', 'dmca', 'court order', ...
    origin_url      TEXT,                   -- where it was first seen; NOT part of the key
    dmca_id         TEXT,                   -- the paperwork, when there is any
    filed_by        TEXT NOT NULL,
    filed_at        TEXT NOT NULL,
    note            TEXT NOT NULL DEFAULT '',
    refusals        INTEGER NOT NULL DEFAULT 0,  -- how many imports it has stopped: a tombstone nobody hits is a guess
    last_refused_at TEXT,
    withdrawn_at    TEXT,
    withdrawn_by    TEXT,
    withdraw_reason TEXT
);

CREATE INDEX work_tombstones_active ON work_tombstones (identity) WHERE withdrawn_at IS NULL;
