-- W5 task 7 — fixity: what the bytes on disk say, against what `content_blobs` claims.
--
-- Two tables, because the two questions are different. A *run* is an event: we looked, at this time, at this
-- many bytes, and stopped for this reason. A *drift* row is a finding that belongs to the run that observed it,
-- which is why a repaired blob does not erase its history: "corrupt on Tuesday, clean on Wednesday" is the
-- answer an operator wants, and a single mutable flag cannot say it.
--
-- No foreign key from `fixity_drift.checksum` to `content_blobs`. A blob row can disappear between the read and
-- the insert (a cache eviction, an erasure), and the finding — this file existed and did not match — must
-- survive the row it described. The finding is evidence about a file, not a reference to a table.
--
-- Every counter is BIGINT: PostgreSQL's INTEGER is 32-bit and the estate has already been bitten once by a
-- count that came back as INT4 where the code expected INT8.
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
    -- Whether the pass reached the end of the store. `complete = 0` with `checked > 0` is a truthful "we ran
    -- out of budget", which a count alone cannot say.
    complete      BIGINT NOT NULL DEFAULT 0,
    error         TEXT
);

CREATE UNIQUE INDEX fixity_runs_seq ON fixity_runs (seq);
CREATE INDEX fixity_runs_started ON fixity_runs (started_at DESC);

CREATE TABLE fixity_drift (
    run_id            TEXT   NOT NULL REFERENCES fixity_runs (id) ON DELETE CASCADE,
    checksum          TEXT   NOT NULL,
    storage_key       TEXT   NOT NULL,
    -- Four kinds, because "wrong" is four facts and an operator acts differently on each: a missing file is a
    -- restore, a size mismatch is a truncation, a hash mismatch is corruption in place, unreadable is a
    -- permission or a device.
    kind              TEXT   NOT NULL CHECK (kind IN ('missing', 'size', 'hash', 'unreadable')),
    expected_bytes    BIGINT NOT NULL,
    actual_bytes      BIGINT,
    actual_checksum   TEXT,
    noted_at          TEXT   NOT NULL,
    PRIMARY KEY (run_id, checksum)
);
