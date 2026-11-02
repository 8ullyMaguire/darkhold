-- 0147 — W2: one account for the estate. Dialect: PostgreSQL. Columns mirror the SQLite file
-- exactly in name and order; the migration-catalogue test asserts it.
--
-- `oidc_links` is the durable fact (subject -> account). `oidc_pending` is one in-flight login, deleted
-- on redemption so a replayed callback finds nothing rather than a second session. No foreign key:
-- accounts.id is UUID while the newer tables keep TEXT ids, and erasure rekeys rather than deletes.
-- local account. `oidc_pending` is one in-flight login, deleted the moment it is redeemed, so a replayed
-- callback finds nothing rather than a second session.
--
-- No foreign key on account_id: `accounts.id` is UUID on PostgreSQL and every id in the newer tables is
-- TEXT (0135/0137 convention), and erasure rekeys rather than deletes, so a key would fire never and
-- fail loudly on the one day it mattered.
--
-- The local password credential is NOT removed when a link is created. §6.4 of the wave document says the
-- form is disabled behind `identity = "local"`; deleting the hash would make rollback a support ticket.

CREATE TABLE IF NOT EXISTS oidc_links (
    provider   TEXT NOT NULL,
    subject    TEXT NOT NULL,
    account_id TEXT NOT NULL,
    email      TEXT,
    linked_at  TEXT NOT NULL,
    PRIMARY KEY (provider, subject)
);

CREATE INDEX IF NOT EXISTS oidc_links_account ON oidc_links (account_id);

CREATE TABLE IF NOT EXISTS oidc_pending (
    state      TEXT NOT NULL PRIMARY KEY,
    verifier   TEXT NOT NULL,
    nonce      TEXT NOT NULL,
    next       TEXT NOT NULL,
    expires_at TEXT NOT NULL
);
