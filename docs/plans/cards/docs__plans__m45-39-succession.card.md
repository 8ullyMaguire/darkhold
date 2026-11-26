# docs/plans/m45-39-succession.md — plan card

Source: `docs/plans/m45-39-succession.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

`docs/requirements.csv`, M45-39, preservation: *"Instance-death protection: mutual backup,
| ActivityPub `Move` | actor-keyed signing; `federation.rs` has no such write | 3 |
- **ActivityPub `Move`.** `routes/federation.rs` reads actors; it has no actor-keyed signed write, so

## Copy-pasteable code / commands

CREATE TABLE succession_plan (
    id           TEXT NOT NULL PRIMARY KEY,   -- the singleton 'instance'
    destination  TEXT NOT NULL,               -- https URL readers should go to
    export_url   TEXT NOT NULL,               -- where a full export can be fetched
    contact      TEXT NOT NULL,               -- mailto: or https URL
    review_days  INTEGER NOT NULL DEFAULT 90,
    confirmed_at TEXT NOT NULL,               -- when the operator last confirmed
    updated_at   TEXT NOT NULL
);

CREATE TABLE succession_redirect (
    id         TEXT NOT NULL PRIMARY KEY,
    from_host  TEXT NOT NULL,                 -- normalised, lowercase, no scheme or port
    from_path  TEXT NOT NULL,                 -- prefix, always leading '/'
    to_url     TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE UNIQUE INDEX succession_redirect_pair ON succession_redirect (from_host, from_path);

cargo test -p darkhold-domain --lib succession        # validation and staleness
cargo test -p darkhold-db  --test succession          # store, both engines
cargo test -p darkhold-app --test m45_39_succession   # the public document and the 404s, both engines
cargo test -p darkhold-app --test route_inventory     # the routes are tabled
cargo test -p darkhold-db  --test migration_catalogue # 0145 is paired

## Verification command and expected output

- **Shortest-prefix instead of longest** → the registry test goes red. Prefix choice is the feature.
- **Width the review window by 100,000 days** → the staleness test goes red. The window is enforcement.
- **Skip the empty-`export_url` branch** → every test stays green, because `!is_https("")` refuses an
cargo test -p darkhold-domain --lib succession        # validation and staleness
cargo test -p darkhold-db  --test succession          # store, both engines
cargo test -p darkhold-app --test m45_39_succession   # the public document and the 404s, both engines
cargo test -p darkhold-app --test route_inventory     # the routes are tabled
cargo test -p darkhold-db  --test migration_catalogue # 0145 is paired

## RED/GREEN evidence

- **Shortest-prefix instead of longest** → the registry test goes red. Prefix choice is the feature.
- **Width the review window by 100,000 days** → the staleness test goes red. The window is enforcement.
- **Skip the empty-`export_url` branch** → every test stays green, because `!is_https("")` refuses an
