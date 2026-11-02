-- 0145 — §11.11a succession: the plan an instance publishes, and the redirect registry (M45-39).
--
-- One row for the plan (`id = 'instance'`). An instance has one succession; a second row would be a
-- second opinion nobody can act on, and two operators editing it is a conflict, not a choice.
--
-- `review_days` rather than a `deadline`: the deadline is `confirmed_at + review_days` and computing
-- it at read time means the document cannot report a date that stopped being true. A stored
-- `is_stale` flag would need something to flip it, and the case it exists for is the case where
-- nothing is running.
--
-- `from_host` is stored normalised (lowercase, no scheme, no port) and `from_path` is a prefix with a
-- leading slash, so resolution is an exact compare on the pair. A wildcard across hosts is a redirect
-- any operator can aim anywhere, which would make this registry a phishing primitive.

CREATE TABLE IF NOT EXISTS succession_plan (
    id           TEXT NOT NULL PRIMARY KEY,
    destination  TEXT NOT NULL,
    export_url   TEXT NOT NULL,
    contact      TEXT NOT NULL,
    review_days  INTEGER NOT NULL DEFAULT 90,
    confirmed_at TEXT NOT NULL,
    updated_at   TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS succession_redirect (
    id         TEXT NOT NULL PRIMARY KEY,
    from_host  TEXT NOT NULL,
    from_path  TEXT NOT NULL,
    to_url     TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS succession_redirect_pair
    ON succession_redirect (from_host, from_path);
