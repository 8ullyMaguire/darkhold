-- 0204 — §E3 rec-post attribution: a share link carries its sharer's code (M45-41). Dialect: PostgreSQL.
--
-- `share_code` is keyed by (account, work) because a reposted work should reuse its link: a second code would pay a second
-- time for the same reader. The code itself is derived from the pair rather than random, so minting is idempotent without a
-- race between two POSTs.
--
-- `share_visit` is the idempotency of the paying: one row per (code, visitor), inserted with ON CONFLICT DO NOTHING, and
-- only the insert that lands pays. A refresh loop inserts nothing and earns nothing -- the alternative (counting reads)
-- makes a credit a function of how often somebody pressed F5.
CREATE TABLE IF NOT EXISTS share_code (
    code       TEXT NOT NULL PRIMARY KEY,
    account_id UUID NOT NULL,
    work_id    UUID NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE (account_id, work_id)
);

CREATE TABLE IF NOT EXISTS share_visit (
    code            TEXT NOT NULL,
    visitor_account UUID NOT NULL,
    credited_at     TEXT NOT NULL,
    PRIMARY KEY (code, visitor_account)
);
