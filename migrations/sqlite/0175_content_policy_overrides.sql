-- 0175_content_policy_overrides — a per-work exception to an instance policy block (M69-16).
--
-- A policy block is an instance deciding not to serve a taxonomy value. An override says: this one work, and no other, is an
-- exception. It exists because a blanket rule of any kind eventually meets the work it was not written for — an archive of a
-- banned trope for scholarly use is the obvious case — and the alternative to a recorded exception is an operator quietly deleting
-- the block, which loses the block's own reason for every other work.
--
-- The override names the **rule it exempts** (`filter_type`, `value`) rather than being a general "allow this work": an exception
-- granted for one block must not silently become an exception for every future block on the same work. That is the difference
-- between an exception and a loophole.
--
-- Same two shape decisions as the blocks themselves: `reason` is NOT NULL, and revocation is a column so the record of "we allowed
-- this for six months, then stopped" survives.
CREATE TABLE IF NOT EXISTS content_policy_overrides (
    id          TEXT PRIMARY KEY,
    work_id     TEXT NOT NULL REFERENCES works (id) ON DELETE CASCADE,
    filter_type TEXT NOT NULL,
    value       TEXT NOT NULL,
    reason      TEXT NOT NULL,
    granted_by  TEXT NOT NULL REFERENCES accounts (id) ON DELETE RESTRICT,
    created_at  TEXT NOT NULL,
    revoked_at  TEXT,
    revoked_by  TEXT REFERENCES accounts (id) ON DELETE RESTRICT,
    revoked_reason TEXT
);

-- One live override per (work, rule).
CREATE UNIQUE INDEX IF NOT EXISTS idx_content_policy_overrides_live
    ON content_policy_overrides (work_id, filter_type, value)
    WHERE revoked_at IS NULL;
