-- M45-77 Step 2 — community trope proposals (PostgreSQL arm).
-- See migrations/sqlite/0219_community_tropes.sql for the ownership notes.

CREATE TABLE IF NOT EXISTS trope_proposals (
    id              UUID PRIMARY KEY,
    slug            TEXT NOT NULL,
    action          TEXT NOT NULL CHECK (action IN ('create','update','retire')),
    payload         TEXT NOT NULL DEFAULT '{}',
    status          TEXT NOT NULL DEFAULT 'proposed'
                    CHECK (status IN ('proposed','passed','active','failed','expired','retired')),
    yes_votes       INTEGER NOT NULL DEFAULT 0,
    no_votes        INTEGER NOT NULL DEFAULT 0,
    quorum_needed   INTEGER NOT NULL,
    closes_at       TIMESTAMPTZ NOT NULL,
    proposed_by     UUID REFERENCES accounts(id) ON DELETE SET NULL,
    decided_by      UUID REFERENCES accounts(id) ON DELETE SET NULL,
    decision_reason TEXT,
    created_at      TIMESTAMPTZ NOT NULL,
    decided_at      TIMESTAMPTZ
);

CREATE UNIQUE INDEX IF NOT EXISTS trope_proposals_live_slug
    ON trope_proposals (slug) WHERE status IN ('proposed', 'active');
CREATE INDEX IF NOT EXISTS trope_proposals_status_idx
    ON trope_proposals (status, closes_at);

CREATE TABLE IF NOT EXISTS trope_votes (
    id          UUID PRIMARY KEY,
    proposal_id UUID NOT NULL REFERENCES trope_proposals(id) ON DELETE CASCADE,
    account_id  UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    value       TEXT NOT NULL CHECK (value IN ('yes','no')),
    created_at  TIMESTAMPTZ NOT NULL,
    UNIQUE (proposal_id, account_id)
);

CREATE INDEX IF NOT EXISTS trope_votes_proposal_idx ON trope_votes (proposal_id);
