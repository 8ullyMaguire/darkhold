-- M45-52 step 1 — see the SQLite migration of the same number for the reasoning.
CREATE TABLE IF NOT EXISTS statement_of_reasons (
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
CREATE INDEX IF NOT EXISTS idx_statement_of_reasons_task ON statement_of_reasons(task_id);
CREATE INDEX IF NOT EXISTS idx_statement_of_reasons_ground ON statement_of_reasons(ground);
