-- W5 task 7 — fixity: what the bytes on disk say, against what `content_blobs` claims.
--
-- The same two tables as the SQLite file, and the filename must match it exactly
-- (`migrations/sqlite/0154_fixity.sql`): `crates/db/src/lib.rs` refuses a dialect pair whose names disagree,
-- because a migration that runs on one backend and not the other is a schema nobody can reason about.
--
-- BIGINT, not INTEGER: this file is where the 32-bit trap lives. SQLite's INTEGER is 64-bit and PostgreSQL's is
-- not, so a count written as INTEGER here comes back as INT4 and a Rust `i64` read of it fails at runtime —
-- which the estate has already paid for once.
--
-- No foreign key from `fixity_drift.checksum` to `content_blobs`: a blob row can disappear between the read and
-- the insert, and the finding — this file existed and did not match — must survive the row it described.
CREATE TABLE fixity_runs (
    id            TEXT   PRIMARY KEY,
    -- A monotonically increasing run number, because "the newest run" must not depend on the clock's precision
    -- or on the sort order of a random uuid. The first version of this table ordered by `started_at` (seconds)
    -- with `id DESC` as a tie-break, and two runs inside one second came back in either order — a test caught
    -- it, which is the only reason it is not in production.
    seq           BIGINT NOT NULL,
    started_at    TEXT   NOT NULL,
    finished_at   TEXT,
    budget_bytes  BIGINT NOT NULL CHECK (budget_bytes >= 0),
    bytes_hashed  BIGINT NOT NULL DEFAULT 0,
    checked       BIGINT NOT NULL DEFAULT 0,
    clean         BIGINT NOT NULL DEFAULT 0,
    drifted       BIGINT NOT NULL DEFAULT 0,
    missing       BIGINT NOT NULL DEFAULT 0,
    complete      BIGINT NOT NULL DEFAULT 0,
    error         TEXT
);

CREATE UNIQUE INDEX fixity_runs_seq ON fixity_runs (seq);
CREATE INDEX fixity_runs_started ON fixity_runs (started_at DESC);

CREATE TABLE fixity_drift (
    run_id            TEXT   NOT NULL REFERENCES fixity_runs (id) ON DELETE CASCADE,
    checksum          TEXT   NOT NULL,
    storage_key       TEXT   NOT NULL,
    kind              TEXT   NOT NULL CHECK (kind IN ('missing', 'size', 'hash', 'unreadable')),
    expected_bytes    BIGINT NOT NULL,
    actual_bytes      BIGINT,
    actual_checksum   TEXT,
    noted_at          TEXT   NOT NULL,
    PRIMARY KEY (run_id, checksum)
);
