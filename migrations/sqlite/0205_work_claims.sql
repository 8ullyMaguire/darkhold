-- 0205 — §E4 claim your works: a claim, its token, and what it decided per work (M45-42).
--
-- `work_claim` is unique on (account, profile_url) because a token is something an author pastes once: a second claim must
-- reuse it, or the author has two strings to keep and no way to know which one the instance is watching for.
--
-- `work_claim_work` carries the OUTCOME per work rather than a boolean, because "the claim attached nothing" is the answer
-- an author cannot act on. NULL means undecided; the two refusals are named values so the door can report them.
CREATE TABLE IF NOT EXISTS work_claim (
    id          TEXT NOT NULL PRIMARY KEY,
    account_id  TEXT NOT NULL,
    profile_url TEXT NOT NULL,
    token       TEXT NOT NULL,
    state       TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    verified_at TEXT,
    UNIQUE (account_id, profile_url)
);

CREATE TABLE IF NOT EXISTS work_claim_work (
    claim_id   TEXT NOT NULL,
    work_id    TEXT NOT NULL,
    outcome    TEXT,
    decided_at TEXT,
    PRIMARY KEY (claim_id, work_id)
);
