-- M63-06: what a saved query answered last time, so a rerun can say what is new.
--
-- One row per view, holding the id set rather than a history: the question is
-- "which of these did I not see last time", and a log would have to be pruned
-- while never being asked anything but this.
--
-- Deliberately not a column on `saved_views`: that row carries an optimistic
-- `version`, and a run that wrote to it would bump the version and invalidate an
-- open editor on every run. The FK cascades, so deleting a view forgets its runs.
CREATE TABLE saved_query_runs (
    saved_view_id TEXT    PRIMARY KEY REFERENCES saved_views (id) ON DELETE CASCADE,
    ran_at        TEXT    NOT NULL,
    seen_ids_json TEXT    NOT NULL,
    seen_count    INTEGER NOT NULL
);
