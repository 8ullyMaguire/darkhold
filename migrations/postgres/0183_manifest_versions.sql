-- M16-07: a package may have more than one version.
--
-- `extension_manifests` was keyed on `id` alone, so a second version of the same package was a UNIQUE
-- violation. That made the review workflow impossible in exactly the case that matters: publishing 1.1.0 over a
-- published 1.0.0 -- and it made "roll back to the previous published version" a promise about a row that could
-- never exist. The key becomes (id, version).
--
-- **This is a rebuild on both dialects, deliberately.** The first version of this file dropped the primary key and added
-- the composite one, which is legal on PostgreSQL and impossible on SQLite. So the two files reached the same key by
-- different routes, and the parity test in `migrate.rs` -- which compares the *declared* schema migration by migration --
-- saw a table on one side and none on the other. It also left PostgreSQL without `idx_extension_manifests_id_version`, an
-- index SQLite created and nothing read. One technique, one schema, whichever engine runs it.
--
-- Every existing row survives: they are unique on (id, version) by construction, because they were unique on `id`.
CREATE TABLE extension_manifests_new (
    id TEXT NOT NULL,               -- slug
    version TEXT NOT NULL,
    document TEXT NOT NULL,         -- the manifest, versioned, signed hash
    submitted_by TEXT NOT NULL,
    state TEXT NOT NULL,            -- pending|approved|rejected|revoked
    created_at TEXT NOT NULL,
    derived_from TEXT,
    PRIMARY KEY (id, version)
);

INSERT INTO extension_manifests_new (id, version, document, submitted_by, state, created_at, derived_from)
SELECT id, version, document, submitted_by, state, created_at, derived_from FROM extension_manifests;

DROP TABLE extension_manifests;

ALTER TABLE extension_manifests_new RENAME TO extension_manifests;

CREATE INDEX idx_extension_manifests_id_version ON extension_manifests(id, version);
