-- 0155: specialist matchmaking (M45-33) — betas, Britpickers, canon consultants, sensitivity readers.
--
-- THREE DECISIONS IN THE SCHEMA
--
-- 1. **A skill is a row, not a string.** `skills = 'beta,britpick'` would make every match a `LIKE '%beta%'`
--    that also matches `betamax`, and a "contains" test cannot use an index. One row per (pseud, skill) is the
--    spelling the query actually wants.
-- 2. **Opt-in is the absence of a row.** Nothing here is created on registration or on a first login; a
--    profile exists because a person wrote one. A reader who never opted in is not "closed", they are absent.
-- 3. **A request names its work and its requester, and carries the brief.** Critique goes only where it is
--    requested: there is no route that opens a request on someone else's work, and no field that a specialist
--    can use to comment without a request existing first.
--
-- `fandoms` stays a comma-separated text column on purpose: it is a *preference* read as a set, never
-- compared by pattern, and it has no join target (fandom names are catalogue vocabulary, not pseud rows).
-- Empty means "any fandom", which is why it is not NULL: the store binds one shape.

CREATE TABLE specialist_profiles (
    pseud_id TEXT PRIMARY KEY REFERENCES pseuds (id) ON DELETE CASCADE,
    fandoms TEXT NOT NULL DEFAULT '',
    capacity TEXT NOT NULL DEFAULT 'open' CHECK (capacity IN ('open', 'closed')),
    note TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE specialist_skills (
    pseud_id TEXT NOT NULL REFERENCES pseuds (id) ON DELETE CASCADE,
    skill TEXT NOT NULL CHECK (skill IN ('beta', 'britpick', 'canon', 'sensitivity')),
    PRIMARY KEY (pseud_id, skill)
);
CREATE INDEX specialist_skills_skill ON specialist_skills (skill);

CREATE TABLE specialist_requests (
    id TEXT PRIMARY KEY,
    work_id TEXT NOT NULL REFERENCES works (id) ON DELETE CASCADE,
    requester_pseud_id TEXT NOT NULL REFERENCES pseuds (id) ON DELETE CASCADE,
    skill TEXT NOT NULL CHECK (skill IN ('beta', 'britpick', 'canon', 'sensitivity')),
    fandom TEXT NOT NULL DEFAULT '',
    brief TEXT NOT NULL DEFAULT '',
    state TEXT NOT NULL DEFAULT 'open' CHECK (state IN ('open', 'matched', 'completed', 'withdrawn')),
    matched_pseud_id TEXT REFERENCES pseuds (id) ON DELETE SET NULL,
    created_at TEXT NOT NULL,
    matched_at TEXT,
    completed_at TEXT
);
CREATE INDEX specialist_requests_open ON specialist_requests (state, skill);
CREATE INDEX specialist_requests_work ON specialist_requests (work_id);
