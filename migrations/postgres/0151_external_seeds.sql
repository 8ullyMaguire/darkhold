-- M45-67: the works a reader wants more of are often not here.
--
-- An external seed is TASTE EVIDENCE, not a catalogue row. `library_items` is what this instance has
-- seen and can report metrics for; a title someone typed from Webnovel is a claim the instance cannot
-- verify, and putting it in `library_items` would make it show up in counts, exports and erasure as if
-- it were real. So it lives in its own table, keeps the words the reader typed, and can be used as a
-- seed without ever being ranked, counted or listed as a work.
--
-- `source_key` is derived from the line (host of a URL, else `unknown`) and `provenance_json` keeps the
-- rest: what was typed, when, by whom. A row here can always answer "why do you think I like this".
CREATE TABLE external_seeds (
    id              TEXT NOT NULL PRIMARY KEY,
    account_id      TEXT NOT NULL,
    spec            TEXT NOT NULL,
    title           TEXT NOT NULL,
    source_key      TEXT NOT NULL,
    source_url      TEXT,
    provenance_json TEXT NOT NULL,
    created_at      TEXT NOT NULL
);

-- One row per (account, spec): re-running a list is not an accumulation of duplicates.
CREATE UNIQUE INDEX external_seeds_owner_spec ON external_seeds (account_id, spec);
CREATE INDEX external_seeds_owner ON external_seeds (account_id, created_at);
