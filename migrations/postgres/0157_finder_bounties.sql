-- 0157: fic-finder bounties — the PostgreSQL twin.
--
-- Same columns and constraints as the SQLite file; target ids are uuid because every PostgreSQL table in this
-- schema names accounts and works that way. `budget` stays INTEGER (i64 on both engines).

CREATE TABLE finder_bounties (
    id                   TEXT PRIMARY KEY,
    asker_account_id     UUID NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    description          TEXT NOT NULL,
    fandom               TEXT,
    budget               BIGINT NOT NULL CHECK (budget > 0),
    state                TEXT NOT NULL DEFAULT 'open'
                         CHECK (state IN ('open', 'accepted', 'cancelled', 'expired')),
    closes_at            TEXT NOT NULL,
    created_at           TEXT NOT NULL,
    closed_at            TEXT,
    accepted_proposal_id TEXT,
    hold_id              TEXT
);
CREATE INDEX finder_bounties_open ON finder_bounties (state, closes_at);
CREATE INDEX finder_bounties_asker ON finder_bounties (asker_account_id);

CREATE TABLE finder_proposals (
    id                  TEXT PRIMARY KEY,
    bounty_id           TEXT NOT NULL REFERENCES finder_bounties (id) ON DELETE CASCADE,
    proposer_account_id UUID NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    work_id             UUID NOT NULL REFERENCES works (id) ON DELETE CASCADE,
    note                TEXT NOT NULL DEFAULT '',
    state               TEXT NOT NULL DEFAULT 'proposed'
                        CHECK (state IN ('proposed', 'accepted', 'rejected', 'withdrawn')),
    created_at          TEXT NOT NULL,
    UNIQUE (bounty_id, proposer_account_id, work_id)
);
CREATE INDEX finder_proposals_bounty ON finder_proposals (bounty_id, state);
