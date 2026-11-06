-- 0177_trope_clusters — trope clusters and their coverage checklist (M69-18).
--
-- A tag count says how much a work names, never what it does not. A cluster is a named family of nodes ("hurt/comfort",
-- "enemies to lovers") with an operator-set priority, and the checklist is derived from it: for one work, which clusters it names at
-- least one node of, and which it never names.
--
-- The checklist is **advisory**. A work is not incomplete for lacking a trope -- it simply knows it has none, and the number is
-- reported to the work's owner (and to a curator), never shown to a stranger as a deficiency.
CREATE TABLE IF NOT EXISTS trope_clusters (
    id          TEXT PRIMARY KEY,
    slug        TEXT NOT NULL UNIQUE,
    label       TEXT NOT NULL,
    description TEXT,
    priority    INTEGER NOT NULL DEFAULT 100,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    version     INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS trope_cluster_nodes (
    cluster_id TEXT NOT NULL REFERENCES trope_clusters (id) ON DELETE CASCADE,
    node_id    TEXT NOT NULL REFERENCES canonical_entities (id) ON DELETE CASCADE,
    PRIMARY KEY (cluster_id, node_id)
);
