-- 0156: stable identifiers for media (M45-37) — identity over location.
--
-- A URL is where a thing was; an identifier is which thing it is. The row's decision (2026-11-05) is that an
-- identifier on a work is a **public catalogue claim**: every reader sees it, and it is how two catalogue rows
-- are recognised as the same book/film/release however many times they were imported.
--
-- WHY THE PRIMARY KEY IS (scheme, value) AND NOT (scheme, value, work)
--
-- Because the whole point is that one identifier names one thing. If two works could both claim the same ISBN,
-- the table would record the ambiguity rather than resolve it, and every lookup would have to pick a winner.
-- The attach path therefore answers "already claimed by work X" instead of inserting a second row — the
-- conflict is the answer, not an error to work around.
--
-- WHY THE TARGET IS TWO COLUMNS WITH AN EXCLUSIVE-OR CHECK
--
-- An ISBN names a *printing*; a MusicBrainz release group names a *work*; a Wikidata item may name either. One
-- nullable pair with `(work_id IS NULL) <> (edition_id IS NULL)` says exactly one target, keeps a single index
-- for lookup, and lets the lookup route answer without knowing which kind it found.
--
-- `value` is the CANONICAL spelling produced by `darkhold_domain::identifier` — never raw input. Storing
-- `978-0-306-40615-7` and `9780306406157` as two rows is precisely the duplicate this table exists to prevent.

CREATE TABLE entity_identifiers (
    scheme     TEXT NOT NULL CHECK (scheme IN ('musicbrainz', 'isrc', 'isbn', 'tmdb', 'wikidata')),
    value      TEXT NOT NULL,
    work_id    TEXT REFERENCES works (id) ON DELETE CASCADE,
    edition_id TEXT REFERENCES media_editions (id) ON DELETE CASCADE,
    -- How it came to be here: imported | curator | quorum | author. Provenance, not decoration — a reader can
    -- weigh "the importer guessed" against "the author said so".
    source     TEXT NOT NULL DEFAULT 'curator' CHECK (source IN ('imported', 'curator', 'quorum', 'author')),
    created_at TEXT NOT NULL,
    PRIMARY KEY (scheme, value),
    CHECK ((work_id IS NULL) <> (edition_id IS NULL))
);
CREATE INDEX entity_identifiers_work ON entity_identifiers (work_id);
CREATE INDEX entity_identifiers_edition ON entity_identifiers (edition_id);
