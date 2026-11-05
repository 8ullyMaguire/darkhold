-- 0156: stable identifiers for media (M45-37) — the PostgreSQL twin.
--
-- Same columns, same constraints, same semantics as the SQLite file; `uuid` for the two targets because every
-- other PostgreSQL table names those rows by uuid. `scheme` and `value` stay TEXT: they are a controlled
-- vocabulary and a canonical string, and casting either to an enum would make a sixth scheme a migration
-- rather than a domain change.

CREATE TABLE entity_identifiers (
    scheme     TEXT NOT NULL CHECK (scheme IN ('musicbrainz', 'isrc', 'isbn', 'tmdb', 'wikidata')),
    value      TEXT NOT NULL,
    work_id    UUID REFERENCES works (id) ON DELETE CASCADE,
    edition_id UUID REFERENCES media_editions (id) ON DELETE CASCADE,
    source     TEXT NOT NULL DEFAULT 'curator' CHECK (source IN ('imported', 'curator', 'quorum', 'author')),
    created_at TEXT NOT NULL,
    PRIMARY KEY (scheme, value),
    CHECK ((work_id IS NULL) <> (edition_id IS NULL))
);
CREATE INDEX entity_identifiers_work ON entity_identifiers (work_id);
CREATE INDEX entity_identifiers_edition ON entity_identifiers (edition_id);
