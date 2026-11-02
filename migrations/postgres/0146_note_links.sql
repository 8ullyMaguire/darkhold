-- 0146 — author-note links are monitored like any other link (M45-38). Dialect: PostgreSQL.
--
-- The reasoning — why `revision_id` owns the row, why the status vocabulary is the existing one, why
-- there are no foreign keys — is in the SQLite file, which is the same table. The one PostgreSQL
-- difference is the index on `last_checked_at`: NULL sorts first there by default, which is what the
-- checker wants (never-checked rows are due immediately), so the index needs no `NULLS LAST` clause
-- and the query needs no `COALESCE`.

CREATE TABLE IF NOT EXISTS note_links (
    id                    TEXT    PRIMARY KEY,
    work_id               TEXT    NOT NULL,
    unit_id               TEXT    NOT NULL,
    revision_id           TEXT    NOT NULL,
    url                   TEXT    NOT NULL,
    host                  TEXT    NOT NULL,
    provider              TEXT    NOT NULL DEFAULT 'other',
    status                TEXT    NOT NULL DEFAULT 'pending_verification',
    -- BIGINT, not INTEGER: ADR 0004 — a value decoded as `i64` in Rust is INTEGER on SQLite and
    -- BIGINT here, and PostgreSQL refuses to decode INT4 into `i64`.
    consecutive_failures  BIGINT  NOT NULL DEFAULT 0,
    -- TEXT, not TIMESTAMPTZ: this workspace's timestamp columns are RFC 3339 text on both engines
    -- (0144's `awarded_at` set the precedent), so one bind works on both without a cast per column.
    last_checked_at       TEXT,
    last_healthy_at       TEXT,
    failure_reason        TEXT,
    created_at            TEXT NOT NULL,
    updated_at            TEXT NOT NULL,
    UNIQUE (revision_id, url)
);

CREATE INDEX IF NOT EXISTS idx_note_links_work ON note_links (work_id, status);
CREATE INDEX IF NOT EXISTS idx_note_links_unit ON note_links (unit_id);
CREATE INDEX IF NOT EXISTS idx_note_links_revision ON note_links (revision_id);
CREATE INDEX IF NOT EXISTS idx_note_links_check ON note_links (last_checked_at, status);
