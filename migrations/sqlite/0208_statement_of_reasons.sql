-- M45-52 step 1: a moderation decision carries a statement of reasons.
--
-- The row's premise, measured: `decide_task` stored an outcome string and an audit entry. The author whose work was
-- acted on was told nothing, and nothing recorded *why* in a form an appeal or a regulator could read. Article 17's
-- category is `ground` (closed set, in `darkhold_domain::governance::StatementGround`); the reviewer's paragraph is
-- `facts`; `legal_ground` names the instrument for the two grounds that are claims about law.
--
-- `appeal_until` is stored rather than derived: a deadline a person was told about must not move when a constant does.
CREATE TABLE statement_of_reasons (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    report_id TEXT,
    outcome TEXT NOT NULL,
    ground TEXT NOT NULL,
    facts TEXT NOT NULL,
    legal_ground TEXT NOT NULL,
    decided_by TEXT NOT NULL,
    decided_at TEXT NOT NULL,
    appeal_until TEXT NOT NULL
);
CREATE INDEX idx_statement_of_reasons_task ON statement_of_reasons(task_id);
CREATE INDEX idx_statement_of_reasons_ground ON statement_of_reasons(ground);
