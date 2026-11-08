-- M45-44: sister instances — subscribing to a peer's public picks.
--
-- §0.4/"sister instances": two instances of this software should be able to tell each other what they are recommending, without a
-- human copying links. The publishing half is a document (`GET /federation/picks`); this migration is the subscribing half, and it
-- is two tables because a subscription and the things it has yielded are different lifetimes: a peer that goes quiet keeps its
-- subscription and its last picks, and a peer whose collection URL changes is re-pointed without re-downloading the same titles.
--
-- Deliberately not `federation_peers`: that table is about *similarity* (a fingerprint comparison with a score), and a
-- subscription is about a URL a human entered and asked to be reminded of. One peer can be both; neither implies the other, and
-- merging them would make "we compute you are similar" and "an operator asked for your picks" the same row.

CREATE TABLE federation_pick_subscriptions (
    id              TEXT PRIMARY KEY,
    -- The host is the identity. A second subscription to the same peer is a changed URL, not a second feed: two feeds from one
    -- host would show every pick twice and there is no way for a reader to tell which row is which.
    peer_host       TEXT NOT NULL UNIQUE,
    collection_url  TEXT NOT NULL,
    -- Off rather than deleted, so turning a peer back on does not lose when it was first trusted.
    enabled         INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    created_at      TEXT NOT NULL,
    last_fetched_at TEXT,
    -- What the last attempt said, in one line. A subscription that has been failing for a week should be visible in the list
    -- rather than only in a log the operator has to know to read.
    last_status     TEXT,
    last_error_at   TEXT
);

CREATE TABLE federation_peer_picks (
    id              TEXT PRIMARY KEY,
    subscription_id TEXT NOT NULL REFERENCES federation_pick_subscriptions (id) ON DELETE CASCADE,
    -- The peer's own URL for the work. It is the identity of a pick: a title can change, a URL is what a reader follows.
    work_url        TEXT NOT NULL,
    title           TEXT NOT NULL,
    author          TEXT,
    fetched_at      TEXT NOT NULL,
    -- One row per work per peer, replaced on re-fetch. A pick that the peer stopped recommending is deleted by the next
    -- successful fetch, so this table is the peer's *current* list and not an archive that grows forever.
    UNIQUE (subscription_id, work_url)
);

CREATE INDEX federation_peer_picks_recent ON federation_peer_picks (fetched_at);
