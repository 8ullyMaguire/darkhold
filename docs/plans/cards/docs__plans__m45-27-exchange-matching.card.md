# docs/plans/m45-27-exchange-matching.md — plan card

Source: `docs/plans/m45-27-exchange-matching.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

`docs/requirements.csv` M45-27 reads *"Exchanges, prompt memes, big bangs with matching engine —
Schema as it stands (`migrations/postgres/0014_events.sql`):
  `rekey_account` in `docs/snapshot-column-policy.json`.
## The matcher (`crates/domain/src/exchanges.rs`, pure, no I/O)
## Doors (`crates/app/src/routes/exchanges.rs`, module `exchanges`)
`source: crates/app/src/routes/exchanges.rs`, no nav (a seasonal surface does not belong in the
| 7 | `python3 scripts/ideas_catalogue.py` | `178 ideas`, 0 homeless |

## Copy-pasteable code / commands

CREATE TABLE events (id TEXT PRIMARY KEY, name TEXT NOT NULL, document TEXT NOT NULL,
                     created_by TEXT NOT NULL, created_at TEXT NOT NULL);
CREATE TABLE event_participation (event_id TEXT NOT NULL, account TEXT NOT NULL,
                                  joined_at TEXT NOT NULL, PRIMARY KEY (event_id, account));

CREATE TABLE exchange_terms (
    event_id TEXT NOT NULL,
    account  TEXT NOT NULL,
    kind     TEXT NOT NULL,   -- 'offers_fandom' | 'wants_fandom' | 'excludes_pseud'
    value    TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (event_id, account, kind, value)
);
CREATE INDEX exchange_terms_lookup ON exchange_terms (event_id, kind, value);

CREATE TABLE exchange_assignments (
    event_id         TEXT NOT NULL,
    giver            TEXT NOT NULL,
    recipient        TEXT NOT NULL,
    assigned_at      TEXT NOT NULL,
    revealed_at      TEXT,
    matched_by       TEXT NOT NULL,   -- 'greedy' | 'augment'; the algorithm that produced it
    PRIMARY KEY (event_id, giver)
);
CREATE UNIQUE INDEX exchange_assignments_recipient ON exchange_assignments (event_id, recipient);

## Verification command and expected output

  is enforced by the database, so a matcher bug cannot ship a double-gift; the test that tries to
  ties therefore resolve by id, not by hashmap iteration. The test asserts two runs over shuffled
| # | command | expected |
| 1 | `cargo test -p darkhold-domain --lib exchanges` | 6 matcher tests pass |
| 2 | `cargo test -p darkhold-db --test exchanges` (SQLite) | 9 store tests pass |
| 3 | `DARKHOLD_TEST_PG_URL=… cargo test -p darkhold-db --test exchanges` | the same 9 pass on PostgreSQL |
| 4 | `cargo test -p darkhold-app --test m45_27_exchanges` | 8 route tests pass |

## RED/GREEN evidence

No RED/GREEN evidence was recorded in the source document.
