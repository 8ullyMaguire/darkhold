-- M66 GS-1 (PostgreSQL). `account_id` is a UUID and cascades: a deleted account cannot leave a consent behind that claims somebody agreed to something. — the consent record, and a guest is a row in `accounts`.
--
-- Spec §14.1 / D-6: a guest is **an account with no credential**, not a shadow table and not a separate cookie island. It
-- gets a pseud, a trust level and an id like any other account, so every later feature (bookmarks, progress, the fold,
-- karma) already has something to point at.
--
-- The consent record exists *before* the session that depends on it, which is the only ordering that makes "consented" a
-- fact about the row rather than a checkbox the frontend remembers. One row per account: consent is re-asked by bumping
-- `version` (config `consent.version`), not by adding a second row, because a table of accepted versions is a table nobody
-- queries correctly at 3am.
CREATE TABLE IF NOT EXISTS consents (
    account_id  UUID    PRIMARY KEY REFERENCES accounts (id) ON DELETE CASCADE,
    version     INTEGER NOT NULL,
    -- The categories accepted, as JSON: `essential` is not optional and is recorded anyway, because the record's job is to
    -- say what the reader was shown, and "everything, including the part that is not a choice" is what they were shown.
    categories  TEXT    NOT NULL DEFAULT '{}',
    accepted_at TEXT    NOT NULL
);
