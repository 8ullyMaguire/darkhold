-- Row 413 — gravity's §13.2 user-list shape, imported (SQLite arm).
--
-- Adapted from `gravity-port/legacy/backend/migrations/20261112000000_user_lists.up.sql`:
--   user_lists            -> the list itself, owner = an account;
--   user_list_members     -> gravity's member row, here an ITEM = a work on a
--                            reading list (darkhold's noun for this shape);
--   user_list_collaborators -> curation rights, role curator or moderator.
-- No `reading_lists` table and no second ownership model: these are gravity's
-- tables under darkhold's identifier and timestamp conventions (ADR 0004).
--
-- Visibility and moderation keep gravity's meanings: public/protected/private,
-- and single_user/chosen_mods/public_quorum. `public_quorum` is storable but
-- refuses every mutation because no per-list quorum engine exists; the service
-- owns that rule, never a route.
--
-- `criteria` is opaque owner-authored JSON text: stored, never evaluated.

CREATE TABLE IF NOT EXISTS user_lists (
    id          TEXT PRIMARY KEY,
    owner_id    TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    name        TEXT NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 120),
    description TEXT NOT NULL DEFAULT '',
    visibility  TEXT NOT NULL DEFAULT 'private'
                CHECK (visibility IN ('public', 'protected', 'private')),
    moderation  TEXT NOT NULL DEFAULT 'single_user'
                CHECK (moderation IN ('single_user', 'chosen_mods', 'public_quorum')),
    criteria    TEXT,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    UNIQUE (owner_id, name)
);

CREATE INDEX IF NOT EXISTS user_lists_owner_idx ON user_lists (owner_id);
CREATE INDEX IF NOT EXISTS user_lists_visibility_idx ON user_lists (visibility)
    WHERE visibility <> 'private';

-- gravity calls these members; in a darkhold reading list a member is a work.
CREATE TABLE IF NOT EXISTS user_list_members (
    list_id    TEXT NOT NULL REFERENCES user_lists(id) ON DELETE CASCADE,
    work_id    TEXT NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    added_by   TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    note       TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    PRIMARY KEY (list_id, work_id)
);

CREATE INDEX IF NOT EXISTS user_list_members_work_idx ON user_list_members (work_id);

CREATE TABLE IF NOT EXISTS user_list_collaborators (
    list_id    TEXT NOT NULL REFERENCES user_lists(id) ON DELETE CASCADE,
    user_id    TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    role       TEXT NOT NULL CHECK (role IN ('curator', 'moderator')),
    created_at TEXT NOT NULL,
    PRIMARY KEY (list_id, user_id)
);

CREATE INDEX IF NOT EXISTS user_list_collaborators_user_idx
    ON user_list_collaborators (user_id);
