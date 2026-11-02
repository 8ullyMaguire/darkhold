-- 0143 — §9.7.9 reading goals (audit item 41, M47-13).
--
-- A goal is PRIVATE, OPT-IN and grants nothing. "Opt-in" is the absence of a row: a reader who has
-- not set a goal has no goal, and there is no state in which one exists but is switched off, so
-- there is no `enabled` column to disagree with the row's existence.
--
-- Keyed on the PSEUD, not the account: a goal is one reader's own pacing, and §9.7.9 calls it a
-- personal progress display. (`reading_status`, which the `works` metric counts, is account-scoped;
-- the store reports that metric's scope rather than pretending it is per-pseud.)
--
-- `target > 0` is a CHECK as well as a Rust bound. A zero-word daily goal is one no reader can meet
-- and no surface can draw, so the database refuses it rather than trusting every future write path.
--
-- The current period is COMPUTED, never stored: a stored window would need a sweep to roll over and
-- would still reset at the wrong hour for most readers. See `domain::reading_goal`.
--
-- There is no `streak` column. A streak is a second feature with a second failure story (a missed
-- day costs nothing, §9.7.1), and nothing in this build computes one.
--
-- Timestamps are canonical `YYYY-MM-DDTHH:MM:SSZ` (M63-31).

CREATE TABLE IF NOT EXISTS reading_goal (
    pseud_id   TEXT NOT NULL PRIMARY KEY,
    period     TEXT NOT NULL CHECK (period IN ('daily', 'weekly')),
    metric     TEXT NOT NULL CHECK (metric IN ('chapters', 'words', 'works')),
    target     INTEGER NOT NULL CHECK (target > 0),
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
