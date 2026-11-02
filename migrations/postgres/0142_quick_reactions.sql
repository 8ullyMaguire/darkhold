-- 0142 — §9.4 quick reactions (audit item 43, M47-12).
--
-- The reasoning (a separate table from `work_reactions`, why the unique index is the requirement,
-- why an unknown label is refused, why no foreign keys) is in the SQLite file and is not repeated.
--
-- Dialect: PostgreSQL.

CREATE TABLE IF NOT EXISTS quick_reaction (
    id         TEXT NOT NULL PRIMARY KEY,
    work_id    TEXT NOT NULL,
    pseud_id   TEXT NOT NULL,
    label      TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS quick_reaction_once
    ON quick_reaction (work_id, pseud_id, label);

CREATE INDEX IF NOT EXISTS quick_reaction_tally
    ON quick_reaction (work_id, label);
