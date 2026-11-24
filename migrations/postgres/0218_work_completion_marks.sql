-- 0218: automatic completion lifecycle (M45-76).
--
-- `completion_source` says who last moved completion. `import` is the honest
-- default for every row that existed before this migration: the importer wrote
-- abandoned rows and this tree has no record of a human decision for any of
-- them.
--
-- `completion_marked_at` is the comparison point for revival. It is seeded from
-- `updated_at` for the existing abandoned rows: an import that loaded a work
-- yesterday did not see a story update after it.

ALTER TABLE works ADD COLUMN completion_source TEXT NOT NULL DEFAULT 'import'
    CHECK (completion_source IN ('import', 'owner', 'review', 'sweep', 'update'));

ALTER TABLE works ADD COLUMN completion_marked_at TEXT;

UPDATE works
   SET completion_marked_at = updated_at
 WHERE completion = 'abandoned' AND completion_marked_at IS NULL;

CREATE TABLE IF NOT EXISTS work_completion_marks (
    id                     UUID PRIMARY KEY,
    work_id                UUID NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    from_completion        TEXT NOT NULL,
    to_completion          TEXT NOT NULL,
    source                 TEXT NOT NULL
                           CHECK (source IN ('import', 'owner', 'review', 'sweep', 'update')),
    actor_account_id       UUID REFERENCES accounts(id) ON DELETE SET NULL,
    note                   TEXT,
    observed_last_activity TEXT,
    marked_at              TEXT NOT NULL,
    CHECK (from_completion IN ('in_progress', 'complete', 'hiatus', 'abandoned')),
    CHECK (to_completion IN ('in_progress', 'complete', 'hiatus', 'abandoned'))
);

CREATE INDEX IF NOT EXISTS work_completion_marks_work_idx
    ON work_completion_marks (work_id, marked_at DESC);
