-- 0142 — §9.4 quick reactions (audit item 43, M47-12).
--
-- §9.4's labelled one-click bar is NOT `work_reactions` (§35.1's typed vote): a reader may vote
-- "insightful" AND react "made me cry" to the same work, and a table that keeps one row per reader
-- cannot say both. Hence a separate table rather than a wider `vote_type`.
--
-- `work_reactions.vote_type` is free text validated in Rust. This one does not repeat that: the
-- label is a slug from the §9.4 vocabulary and an unknown one is refused, because a free-text
-- column is how a bar fills with typos that each count once and can never be joined to anything.
--
-- The unique index IS the requirement. §9.4 says "count once per reader per target", so the
-- invariant lives in the schema instead of in a `COUNT(DISTINCT …)` that a later query forgets.
-- Re-reacting is ON CONFLICT DO NOTHING: a double-tap is one reaction, not an error.
--
-- No foreign keys, following 0137: §51 erases an account by REKEYING its pseud, so a cascade would
-- never fire, and an orphan reaction on a deleted work simply stops appearing because every read
-- joins `works`.
--
-- Timestamps are canonical `YYYY-MM-DDTHH:MM:SSZ` (M63-31).

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
