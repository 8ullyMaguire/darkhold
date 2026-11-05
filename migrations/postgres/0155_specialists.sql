-- 0155: specialist matchmaking (M45-33) — the PostgreSQL twin.
--
-- Columns and constraints are the SQLite file's, with the estate's PostgreSQL types: `uuid` for the two
-- identifiers, so that a pseud and a work compare as the rows they name. The store binds them with `::uuid`
-- casts; `fandom` and `brief` stay TEXT, and no timestamp is `timestamptz` (the storage contract is RFC 3339
-- text on every table, and a column that is a different type on one dialect is a query that works on one).

CREATE TABLE specialist_profiles (
    pseud_id UUID PRIMARY KEY REFERENCES pseuds (id) ON DELETE CASCADE,
    fandoms TEXT NOT NULL DEFAULT '',
    capacity TEXT NOT NULL DEFAULT 'open' CHECK (capacity IN ('open', 'closed')),
    note TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE specialist_skills (
    pseud_id UUID NOT NULL REFERENCES pseuds (id) ON DELETE CASCADE,
    skill TEXT NOT NULL CHECK (skill IN ('beta', 'britpick', 'canon', 'sensitivity')),
    PRIMARY KEY (pseud_id, skill)
);
CREATE INDEX specialist_skills_skill ON specialist_skills (skill);

CREATE TABLE specialist_requests (
    id UUID PRIMARY KEY,
    work_id UUID NOT NULL REFERENCES works (id) ON DELETE CASCADE,
    requester_pseud_id UUID NOT NULL REFERENCES pseuds (id) ON DELETE CASCADE,
    skill TEXT NOT NULL CHECK (skill IN ('beta', 'britpick', 'canon', 'sensitivity')),
    fandom TEXT NOT NULL DEFAULT '',
    brief TEXT NOT NULL DEFAULT '',
    state TEXT NOT NULL DEFAULT 'open' CHECK (state IN ('open', 'matched', 'completed', 'withdrawn')),
    matched_pseud_id UUID REFERENCES pseuds (id) ON DELETE SET NULL,
    created_at TEXT NOT NULL,
    matched_at TEXT,
    completed_at TEXT
);
CREATE INDEX specialist_requests_open ON specialist_requests (state, skill);
CREATE INDEX specialist_requests_work ON specialist_requests (work_id);
