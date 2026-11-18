-- Row 413 — gravity's §13.2 user-list shape, imported (PostgreSQL arm).
-- See migrations/sqlite/0217_user_lists.sql for the adaptation notes: the
-- member row holds a work (a reading-list item), and the service owns every
-- visibility and mutation gate.

CREATE TABLE IF NOT EXISTS user_lists (
    id          UUID PRIMARY KEY,
    owner_id    UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    name        TEXT NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 120),
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

CREATE TABLE IF NOT EXISTS user_list_members (
    list_id    UUID NOT NULL REFERENCES user_lists(id) ON DELETE CASCADE,
    work_id    UUID NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    added_by   UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    note       TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    PRIMARY KEY (list_id, work_id)
);

CREATE INDEX IF NOT EXISTS user_list_members_work_idx ON user_list_members (work_id);

CREATE TABLE IF NOT EXISTS user_list_collaborators (
    list_id    UUID NOT NULL REFERENCES user_lists(id) ON DELETE CASCADE,
    user_id    UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    role       TEXT NOT NULL CHECK (role IN ('curator', 'moderator')),
    created_at TEXT NOT NULL,
    PRIMARY KEY (list_id, user_id)
);

CREATE INDEX IF NOT EXISTS user_list_collaborators_user_idx
    ON user_list_collaborators (user_id);
