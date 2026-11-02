-- 0143 — §9.7.9 reading goals (audit item 41, M47-13).
--
-- The reasoning (pseud-keyed, opt-in as absence of a row, why `target > 0` is a CHECK, why the
-- period is computed rather than stored, why there is no streak column) is in the SQLite file and
-- is not repeated here.
--
-- Dialect: PostgreSQL.

CREATE TABLE IF NOT EXISTS reading_goal (
    pseud_id   TEXT NOT NULL PRIMARY KEY,
    period     TEXT NOT NULL CHECK (period IN ('daily', 'weekly')),
    metric     TEXT NOT NULL CHECK (metric IN ('chapters', 'words', 'works')),
    -- BIGINT, not INTEGER: ADR 0004 — a value decoded as `i64` in Rust is INTEGER on SQLite and
    -- BIGINT here, and PostgreSQL refuses to decode INT4 into `i64`. The second engine found this
    -- one as a runtime decode error, which is the least pleasant way to learn it.
    target     BIGINT NOT NULL CHECK (target > 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- 0143b — which chapters a reader has reached, because nothing recorded it.
--
-- PROBED, not assumed: `reading_progress` is unique on
-- (account_id, pseud_id, subject_type, subject_id) — one row per WORK per device — so it holds the
-- chapter the reader is ON, not the chapters they have read. A "chapters" goal cannot be counted
-- from it: three chapters of one work are one row. §9.7.9 offers a chapters target, so the record it
-- needs did not exist and is added here rather than quietly redefining the metric as "works in
-- flight".
--
-- One row per reader per chapter, `last_read_at` moved forward on every read: "chapters read in the
-- window" is then a plain count, and re-reading a chapter next week counts it in that week without
-- ever counting it twice in one.
--
-- No foreign keys, following 0137 (erasure rekeys a pseud, so a cascade never fires).

CREATE TABLE IF NOT EXISTS reading_unit_read (
    id            TEXT NOT NULL PRIMARY KEY,
    pseud_id      TEXT NOT NULL,
    work_id       TEXT NOT NULL,
    unit_id       TEXT NOT NULL,
    first_read_at TEXT NOT NULL,
    last_read_at  TEXT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS reading_unit_read_once
    ON reading_unit_read (pseud_id, unit_id);

CREATE INDEX IF NOT EXISTS reading_unit_read_window
    ON reading_unit_read (pseud_id, last_read_at);
