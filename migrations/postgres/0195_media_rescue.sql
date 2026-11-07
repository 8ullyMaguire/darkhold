-- M69-06: the rescue ring. What a survivor says it holds, and what the ring decided when a peer died.
--
-- §16.4. Perceptual hashes have existed since M32-07 and local duplicate *proposals* since M32-07e; neither is a rescue ring.
-- A local proposal pairs two rows this instance already has. A rescue asks whether anybody *else* still has the bytes for
-- media whose only holder went dark.
--
-- Two tables, because the two facts have different lifetimes: `media_rescue_copies` is what a survivor claims (refreshed,
-- re-supplied, never a promise that it still serves it), and `media_rescue_requests` is one hunt with its outcome. The
-- outcome is stored rather than recomputed because a rescue is a historical fact, and a later copy arriving must not rewrite
-- what an operator was told at the time.
--
-- An ambiguous outcome stores NO matched instance: writing one of the two tied survivors would be the ring choosing, which
-- is the one thing it must not do.
CREATE TABLE media_rescue_copies (
    id               TEXT PRIMARY KEY,
    instance         TEXT NOT NULL,          -- the survivor's canonical base URL
    content_hash     TEXT NOT NULL,          -- exact bytes, when the survivor reports them
    perceptual_hash  TEXT,
    width            INTEGER,
    height           INTEGER,
    duration_seconds INTEGER,
    media_kind       TEXT NOT NULL DEFAULT 'image',
    recorded_at      TEXT NOT NULL,
    last_seen_at     TEXT,
    UNIQUE (instance, content_hash)
);

CREATE INDEX media_rescue_copies_content ON media_rescue_copies (content_hash);
CREATE INDEX media_rescue_copies_perceptual ON media_rescue_copies (perceptual_hash);

CREATE TABLE media_rescue_requests (
    id                  TEXT PRIMARY KEY,
    dead_peer           TEXT NOT NULL,       -- whose copy is gone
    media_reference_id  TEXT NOT NULL,       -- the reference this instance still has a row for
    content_hash        TEXT NOT NULL,
    perceptual_hash     TEXT,
    threshold           INTEGER NOT NULL,    -- the Hamming bound the search used, recorded with the outcome
    outcome             TEXT NOT NULL
                        CHECK (outcome IN ('exact', 'proposal', 'ambiguous', 'not_found')),
    matched_instance    TEXT,                -- null unless exactly one survivor was named
    distance            INTEGER,
    confidence          REAL,
    candidates_json     TEXT NOT NULL DEFAULT '[]', -- the tied or near names, so a human can choose later
    created_at          TEXT NOT NULL,
    resolved_at         TEXT,
    resolved_by         TEXT,
    resolution          TEXT                 -- 'accepted' | 'rejected' | 'abandoned', null while pending
);

CREATE INDEX media_rescue_requests_status ON media_rescue_requests (resolved_at, created_at);
CREATE INDEX media_rescue_requests_dead_peer ON media_rescue_requests (dead_peer);
