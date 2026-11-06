-- 0176_tag_scripts — see the SQLite migration for the reasoning.
CREATE TABLE IF NOT EXISTS tag_scripts (
    id          UUID PRIMARY KEY,
    account_id  UUID NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL,
    version     INTEGER NOT NULL DEFAULT 1,
    UNIQUE (account_id, name)
);

CREATE TABLE IF NOT EXISTS tag_script_nodes (
    script_id UUID NOT NULL REFERENCES tag_scripts (id) ON DELETE CASCADE,
    node_id   TEXT NOT NULL REFERENCES canonical_entities (id) ON DELETE CASCADE,
    position  INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (script_id, node_id)
);
