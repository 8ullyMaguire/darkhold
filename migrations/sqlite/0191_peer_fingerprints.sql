-- M69-02: what a peer published about itself, kept so peering can be suggested without asking it again.
--
-- The document is stored whole (JSON) rather than shredded into columns: it is a *claim* the peer made at a moment, and
-- the shape it has is the shape the peer's own build emitted. Shredding it would mean a reader that can read this
-- instance's fingerprint and not a peer's.

CREATE TABLE peer_fingerprints (
    instance_id TEXT PRIMARY KEY,   -- the peer's canonical base URL, as its document spells it
    document    TEXT NOT NULL,      -- the whole `/.well-known/taste-fingerprint.json` body
    fetched_at  TEXT NOT NULL
);
