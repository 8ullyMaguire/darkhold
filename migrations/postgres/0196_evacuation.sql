-- M69-07: the evacuation notice, the plan it produced, and the receipt that says who took what.
--
-- §16.4. `work_mirrors` (0194) records that a peer holds a copy *now*; this records something shorter-lived: an instance has
-- announced it is going away, and these works were handed to these recipients. The two must not be merged -- a mirror can be
-- retired and re-armed any number of times, and an evacuation is a one-way event with a receipt.
--
-- `UNIQUE (notice_id, work_id)` is the anti-fork rule at the database level: one work, one planned recipient. The *receipt* is
-- the second half -- a recipient may still claim a work it was not offered, which is why confirming an acceptance refuses a
-- second one for the same work rather than trusting the plan.
--
-- No `account_id` anywhere: these are rows about works and hosts, not about people.

CREATE TABLE evacuation_notices (
    id          TEXT PRIMARY KEY,
    reason      TEXT NOT NULL,
    announced_by TEXT NOT NULL,
    announced_at TEXT NOT NULL,
    state       TEXT NOT NULL DEFAULT 'announced'
                CHECK (state IN ('announced', 'withdrawn', 'completed')),
    note        TEXT NOT NULL DEFAULT '',
    completed_at TEXT
);

-- One open announcement at a time. A second `announced` row would mean two plans for one shutdown, which is the fork one
-- level up from a work forking.
CREATE UNIQUE INDEX evacuation_notices_open ON evacuation_notices (state) WHERE state = 'announced';

CREATE TABLE evacuation_assignments (
    id                 TEXT PRIMARY KEY,
    notice_id          TEXT NOT NULL,
    work_id            TEXT NOT NULL,
    title              TEXT NOT NULL,
    recipient_instance TEXT NOT NULL,
    similarity_bp      INTEGER NOT NULL,
    state              TEXT NOT NULL DEFAULT 'offered'
                       CHECK (state IN ('offered', 'accepted', 'declined')),
    offered_at         TEXT NOT NULL,
    settled_at         TEXT,
    settle_note        TEXT,
    UNIQUE (notice_id, work_id)
);

CREATE INDEX evacuation_assignments_notice ON evacuation_assignments (notice_id, state);
CREATE INDEX evacuation_assignments_recipient ON evacuation_assignments (recipient_instance);
