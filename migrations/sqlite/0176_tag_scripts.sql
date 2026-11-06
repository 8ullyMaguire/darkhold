-- 0176_tag_scripts — a persistent set of tag assertions applied while browsing (M69-20).
--
-- Tagging a work one form at a time is a chore, and chores get done in batches: a reader reading through forty works of one
-- fandom knows twenty of them share three tags, and typing those three tags forty times is the reason the tag stays missing. A
-- script is that set, kept, so applying it is one gesture while browsing rather than a form.
--
-- It is a **set of assertions**, not a saved filter: applying one writes `work_tags` rows through `tag_assertions::assert_tag` with
-- `Claimant::Human`, so everything M69-17 established holds — the row records a person as its claimant, and a script cannot write
-- an inference.
--
-- Membership is a child table rather than a JSON array so a node can be added and removed with one statement, and so a deleted
-- taxonomy node takes its membership with it instead of leaving a dead string inside a document.
CREATE TABLE IF NOT EXISTS tag_scripts (
    id          TEXT PRIMARY KEY,
    account_id  TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    version     INTEGER NOT NULL DEFAULT 1,
    UNIQUE (account_id, name)
);

CREATE TABLE IF NOT EXISTS tag_script_nodes (
    script_id TEXT NOT NULL REFERENCES tag_scripts (id) ON DELETE CASCADE,
    node_id   TEXT NOT NULL REFERENCES canonical_entities (id) ON DELETE CASCADE,
    position  INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (script_id, node_id)
);
