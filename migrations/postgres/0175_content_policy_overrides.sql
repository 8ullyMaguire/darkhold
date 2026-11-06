-- 0175_content_policy_overrides — see the SQLite migration for the reasoning.
CREATE TABLE IF NOT EXISTS content_policy_overrides (
    id          UUID PRIMARY KEY,
    work_id     UUID NOT NULL REFERENCES works (id) ON DELETE CASCADE,
    filter_type TEXT NOT NULL,
    value       TEXT NOT NULL,
    reason      TEXT NOT NULL,
    granted_by  UUID NOT NULL REFERENCES accounts (id) ON DELETE RESTRICT,
    created_at  TIMESTAMPTZ NOT NULL,
    revoked_at  TIMESTAMPTZ,
    revoked_by  UUID REFERENCES accounts (id) ON DELETE RESTRICT,
    revoked_reason TEXT
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_content_policy_overrides_live
    ON content_policy_overrides (work_id, filter_type, value)
    WHERE revoked_at IS NULL;
