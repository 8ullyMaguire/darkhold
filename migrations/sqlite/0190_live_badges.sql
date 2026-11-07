-- M69-14: a badge that is held while a relationship holds, and lost when it does not.
--
-- `badge_award` cannot carry this: it is unique per (account, badge) and its whole point is that a badge is a one-time
-- fact with a credit bonus. Attuned is a *state*. Two tables, because the state and the history answer different
-- questions: `live_badge_state` says what a reader holds now (one row per badge), and `live_badge_events` says when it
-- changed and why. A badge that vanished with no row here would be indistinguishable from a bug.

CREATE TABLE live_badge_state (
    account_id  TEXT NOT NULL,
    badge       TEXT NOT NULL,
    held        INTEGER NOT NULL DEFAULT 0,
    since       TEXT,                 -- when it was gained; cleared on loss, because "since when" means "held since"
    checked_at  TEXT NOT NULL,
    affinity_bp INTEGER,              -- the reading that decided it; NULL means unmeasured, never 0
    evidence_bp INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (account_id, badge)
);

CREATE TABLE live_badge_events (
    -- `seq` and not `id`: a loss immediately after a gain carries the same second, so ordering by `at` alone would leave
    -- "newest first" to the tie-break, and a reader's history would sometimes show the gain above the loss. A sequence is
    -- the order things happened in, which is what a history is.
    seq         INTEGER PRIMARY KEY AUTOINCREMENT,
    id          TEXT NOT NULL UNIQUE,
    account_id  TEXT NOT NULL,
    badge       TEXT NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN ('gained', 'lost')),
    affinity_bp INTEGER,              -- NULL when the loss was "no reading at all"
    evidence_bp INTEGER NOT NULL,
    reason      TEXT NOT NULL,
    at          TEXT NOT NULL
);

CREATE INDEX live_badge_events_account ON live_badge_events (account_id, seq DESC);
