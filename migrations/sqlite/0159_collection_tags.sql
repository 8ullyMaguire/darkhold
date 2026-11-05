-- M70-05 — collection tags: what a collection is FOR, not what it contains.
--
-- A collection tag describes the collection's intent — a ship week, a challenge, a reading club — so it is a fact
-- about the collection. It must not become a fact about the works inside it: if it did, tagging a challenge would
-- push every work in it into that tag's search results and into readers' taste vectors, which is a claim nobody
-- made.
--
-- That separation is made structural rather than conditional: this table is the only place collection tags live, and
-- the work search, tag listing and taste paths read `work_tags`. Nothing has to remember to exclude it.
--
-- The tag itself is a `canonical_entities` row, so canonicalisation is shared with every other tag in the product rather
-- than reimplemented here.
CREATE TABLE IF NOT EXISTS collection_tags (
    collection_id TEXT NOT NULL REFERENCES collections (id) ON DELETE CASCADE,
    node_id       TEXT NOT NULL REFERENCES canonical_entities (id) ON DELETE CASCADE,
    added_at      TEXT NOT NULL,
    PRIMARY KEY (collection_id, node_id)
);

CREATE INDEX IF NOT EXISTS collection_tags_node ON collection_tags (node_id);
