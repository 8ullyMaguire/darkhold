
-- GS-5 — the drawn panel, and the verdicts that close it.
--
-- A panel is drawn FROM a pool and must be re-readable months later ("show me the draw"), so the seed is stored beside the members: recomputing the
-- draw needs the same pool, and the pool moves. `size` and `level_floor` are stored rather than re-read from config for the same reason — a config
-- change must not rewrite what a past panel was.
--
-- `appeal_of` names the panel an appeal replaced. `governance.appeal_fresh_jury` makes the appeal panel exclude the earlier members, and exclusion
-- needs the earlier panel to exist as a row; a rule that excludes "whoever was on it" with nothing stored excludes nobody.
CREATE TABLE IF NOT EXISTS jury_panels (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    seed TEXT NOT NULL,
    size INTEGER NOT NULL,
    level_floor INTEGER NOT NULL,
    drawn_at TEXT NOT NULL,
    appeal_of TEXT
);
CREATE INDEX IF NOT EXISTS idx_jury_panels_task ON jury_panels(task_id);

-- One row per seated juror. `verdict` is empty until the juror speaks; a panel closes when every member has one, and `record_verdict` refuses a
-- second answer from the same juror rather than letting a juror change their mind silently.
CREATE TABLE IF NOT EXISTS jury_panel_members (
    panel_id TEXT NOT NULL,
    account TEXT NOT NULL,
    verdict TEXT NOT NULL DEFAULT '',
    verdict_at TEXT,
    PRIMARY KEY (panel_id, account)
);
