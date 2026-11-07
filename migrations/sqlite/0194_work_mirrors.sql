-- M69-05: works this instance hands to a peer, and the attribution a reader sees on the work page.
--
-- §16.4 ("preserved by"). `federated_mirrors` (0063) tracks *media* copies by `media_reference_id`; nothing in the tree has
-- ever written a row to it. This is the work-level record, and it exists for one reason: a copy on another host is only
-- honest if the page that serves it says so, and that needs to be a fact stored next to the work rather than an operator's
-- memory.
--
-- The similarity is stored, not recomputed on read. A reader asking "why is this mirrored there?" must get the answer the
-- decision was made on, not today's number after the peer's taste drifted; the same reason `arena` records the round it
-- played rather than re-deriving it.
--
-- `state` rather than deletion: a mirror that was withdrawn or lost is still a fact about this work having been somewhere
-- else, and a reader who saw the line yesterday is owed an explanation, not silence.

CREATE TABLE work_mirrors (
    id              TEXT PRIMARY KEY,
    work_id         TEXT NOT NULL,
    peer_instance   TEXT NOT NULL,          -- the peer's canonical base URL, as its fingerprint declares it
    peer_url        TEXT NOT NULL DEFAULT '', -- where a reader finds the copy there; empty when the peer gave no address
    similarity_bp   INTEGER NOT NULL,       -- taste similarity at the moment of the decision, basis points
    state           TEXT NOT NULL DEFAULT 'active'
                    CHECK (state IN ('active', 'lost', 'withdrawn')),
    mirrored_at     TEXT NOT NULL,
    last_seen_at    TEXT,
    retired_at      TEXT,
    retire_reason   TEXT,
    UNIQUE (work_id, peer_instance)
);

CREATE INDEX work_mirrors_work ON work_mirrors (work_id, state);
CREATE INDEX work_mirrors_peer ON work_mirrors (peer_instance);
