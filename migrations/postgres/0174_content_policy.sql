-- 0174_content_policy — see the SQLite migration for the reasoning.
CREATE TABLE IF NOT EXISTS content_policy_blocks (
    id          UUID PRIMARY KEY,
    filter_type TEXT NOT NULL,
    value       TEXT NOT NULL,
    reason      TEXT NOT NULL,
    created_by  UUID NOT NULL REFERENCES accounts (id) ON DELETE RESTRICT,
    created_at  TIMESTAMPTZ NOT NULL,
    revoked_at  TIMESTAMPTZ,
    revoked_by  UUID REFERENCES accounts (id) ON DELETE RESTRICT,
    revoked_reason TEXT
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_content_policy_live
    ON content_policy_blocks (filter_type, value)
    WHERE revoked_at IS NULL;
