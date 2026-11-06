-- 0177_trope_clusters — see the SQLite migration for the reasoning.
CREATE TABLE IF NOT EXISTS trope_clusters (
    id          UUID PRIMARY KEY,
    slug        TEXT NOT NULL UNIQUE,
    label       TEXT NOT NULL,
    description TEXT,
    priority    INTEGER NOT NULL DEFAULT 100,
    created_at  TIMESTAMPTZ NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL,
    version     INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS trope_cluster_nodes (
    cluster_id UUID NOT NULL REFERENCES trope_clusters (id) ON DELETE CASCADE,
    node_id    TEXT NOT NULL REFERENCES canonical_entities (id) ON DELETE CASCADE,
    PRIMARY KEY (cluster_id, node_id)
);
