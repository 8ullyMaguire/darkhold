-- 0174_content_policy — the instance-level content policy (M69-16, layer two of three).
--
-- The three layers, and why only the middle one is a table:
--
--   1. HARDCODED — the SQL every read already carries: `lifecycle = 'published' AND visibility = 'public' AND deleted_at IS
--      NULL`. No configuration can reach these, which is what makes them the hardcoded layer; they are invariants, not policy.
--   2. POLICY — this table. An operator blocks a taxonomy value for the whole instance and says why.
--   3. TASTE — `content_filters`. The reader's own blocks, per pseud.
--
-- `reason` is NOT NULL on purpose. A policy block is an instance taking a decision about what its readers may see; a row that
-- cannot say why is a row nobody can defend or revoke with confidence. Revocation is a column rather than a delete so the record
-- of the block survives the block itself — that record is the audit trail, and deleting it would erase the only evidence that the
-- instance once chose to hide something.
CREATE TABLE IF NOT EXISTS content_policy_blocks (
    id          TEXT PRIMARY KEY,
    filter_type TEXT NOT NULL,
    value       TEXT NOT NULL,
    reason      TEXT NOT NULL,
    created_by  TEXT NOT NULL REFERENCES accounts (id) ON DELETE RESTRICT,
    created_at  TEXT NOT NULL,
    revoked_at  TEXT,
    revoked_by  TEXT REFERENCES accounts (id) ON DELETE RESTRICT,
    revoked_reason TEXT
);

-- One live block per (type, value). Partial, so a revoked block does not prevent re-blocking the same value later while leaving
-- the earlier record intact.
CREATE UNIQUE INDEX IF NOT EXISTS idx_content_policy_live
    ON content_policy_blocks (filter_type, value)
    WHERE revoked_at IS NULL;
