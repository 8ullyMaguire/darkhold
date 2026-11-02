-- 0145 — §11.11a succession: the plan an instance publishes, and the redirect registry (M45-39).
--
-- The reasoning (one row, `review_days` rather than a stored deadline, normalised host, exact pair)
-- is in the SQLite file. Dialect: PostgreSQL.
--
-- `review_days` is BIGINT: ADR 0004 — the value travels as `i64`, and PostgreSQL will not store an
-- INT8 bind in an INTEGER column ("column is of type integer but expression is of type bigint"),
-- while SQLite's INTEGER is already 64-bit. The first version of this file said INTEGER and the
-- route returned 500 on PostgreSQL only.

CREATE TABLE IF NOT EXISTS succession_plan (
    id           TEXT NOT NULL PRIMARY KEY,
    destination  TEXT NOT NULL,
    export_url   TEXT NOT NULL,
    contact      TEXT NOT NULL,
    review_days  BIGINT NOT NULL DEFAULT 90,
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
