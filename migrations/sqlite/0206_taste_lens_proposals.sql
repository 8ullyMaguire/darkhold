-- 0206 — a taste-lens change as a proposal the instance votes on (M45-45, step 3).
--
-- The lens is a singleton (`instance_taste_settings` has `CHECK (id = 1)`), so a proposal is not about a *key*: it is
-- about a set of knobs that would replace the row if it passed. That is why this is a sibling of `retention_proposals`
-- rather than a row in it — sharing one table would put `WHERE kind = …` in every query and one of them would forget.
--
-- `changed_by` on the applied version records `proposal:<id>`, so the history says WHY a version changed and not only
-- who pressed the button.
CREATE TABLE IF NOT EXISTS taste_lens_proposals (
    id                          TEXT NOT NULL PRIMARY KEY,
    gravity_strength            INTEGER NOT NULL,
    signal_weight_mode          TEXT NOT NULL,
    admin_weight                INTEGER NOT NULL,
    diversity_injection_percent INTEGER NOT NULL,
    opened_by                   TEXT NOT NULL,
    opened_at                   TEXT NOT NULL,
    closes_at                   TEXT NOT NULL,
    -- Percentage of the votes cast that must be `yes`. Stored per proposal: the threshold that was in force when the
    -- proposal opened is the one that decides it, so changing the default later cannot retroactively pass an old vote.
    threshold_percent           INTEGER NOT NULL,
    state                       TEXT NOT NULL DEFAULT 'open',
    decided_at                  TEXT
);

CREATE TABLE IF NOT EXISTS taste_lens_votes (
    proposal_id  TEXT NOT NULL REFERENCES taste_lens_proposals (id) ON DELETE CASCADE,
    voter_account TEXT NOT NULL,
    vote         TEXT NOT NULL,
    cast_at      TEXT NOT NULL,
    -- One vote per account per proposal. A second vote replaces the first rather than adding a second: a reader who
    -- changes their mind must not be worth two.
    PRIMARY KEY (proposal_id, voter_account)
);
