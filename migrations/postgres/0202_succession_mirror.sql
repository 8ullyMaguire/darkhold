-- 0202 — §11.11a's missing half: the peer that holds a copy (M45-39). Dialect: PostgreSQL.
--
-- The published document already carried `not_built`, and one of the two entries was "mutual backup with a peer instance".
-- That confession is honest only as long as nothing records a backup, which is why the row is here rather than in a doc.
--
-- `peer_id` is the primary key because two rows for one peer would be two claims about one copy: the second is a correction
-- of the first (a moved export URL), never an addition. `asserted_at` is when the OPERATOR said so; this instance cannot
-- reach into a peer's disk, so the document says `verified: false` instead of implying it checked.
CREATE TABLE IF NOT EXISTS succession_mirror (
    peer_id     TEXT NOT NULL PRIMARY KEY,
    export_url  TEXT NOT NULL,
    asserted_at TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
