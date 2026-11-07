-- M69-06: the rescue ring. What a survivor says it holds, and what the ring decided when a peer died.
--
-- §16.4. darkhold has computed perceptual hashes since M32-07 and has proposed *local* duplicate matches since M32-07e.
-- Neither is a rescue ring: a local proposal pairs two rows this instance already has, and a rescue asks whether anybody
-- *else* still has the bytes for media whose only holder went dark.
--
-- Two tables, because the two facts have different lifetimes:
--
--   * `media_rescue_copies` is what a survivor claims. It is refreshed and re-supplied; it is not evidence that the instance
--     still serves it, only that it said so, which is why `last_seen_at` exists and nothing joins on it as a promise.
--   * `media_rescue_requests` is one hunt: media lost at a named dead peer, and the outcome. The outcome is stored rather
--     than recomputed because a rescue is a historical fact — "we looked, and here is what we found, on this date" — and a
--     later copy arriving must not rewrite what the operator was told at the time.
--
-- An ambiguous outcome stores NO matched instance. The columns are nullable for exactly that reason: writing one of the two
-- tied survivors would be the ring choosing, which is the one thing it must not do.

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
