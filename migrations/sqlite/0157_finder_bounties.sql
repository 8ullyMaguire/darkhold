-- 0157: fic-finder bounties (M45-34).
--
-- A bounty is a question with credits behind it: "find this half-remembered fic and I will pay you". The
-- credits are reserved when the bounty opens (`credit_holds`, which is the existing escrow primitive) and only
-- move when the asker accepts one answer.
--
-- WHY `accepted_proposal_id` IS A COLUMN AND NOT JUST THE PROPOSAL'S STATE
--
-- Two writes have to agree: the bounty closes and one proposal wins. Storing the winner on the bounty means a
-- single row carries the outcome, so a reader cannot see "accepted" with no winning proposal; and the CAS on
-- `state = 'open'` uses it, which is what makes two simultaneous accepts resolve to one winner.
--
-- WHY PROPOSALS ARE UNIQUE PER (bounty, proposer, work)
--
-- A proposer saying the same thing twice is one answer posted twice, and it would otherwise let one person
-- occupy every slot in an answer list. Different works from the same proposer remain allowed: two candidates
-- are a real answer.
--
-- `state` on a proposal is TEXT with a CHECK rather than an enum, matching every other state column here.

CREATE TABLE finder_bounties (
    id                   TEXT PRIMARY KEY,
    asker_account_id     TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    description          TEXT NOT NULL,
    fandom               TEXT,
    budget               INTEGER NOT NULL CHECK (budget > 0),
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
    proposer_account_id TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    work_id             TEXT NOT NULL REFERENCES works (id) ON DELETE CASCADE,
    note                TEXT NOT NULL DEFAULT '',
    state               TEXT NOT NULL DEFAULT 'proposed'
                        CHECK (state IN ('proposed', 'accepted', 'rejected', 'withdrawn')),
    created_at          TEXT NOT NULL,
    UNIQUE (bounty_id, proposer_account_id, work_id)
);
CREATE INDEX finder_proposals_bounty ON finder_proposals (bounty_id, state);
