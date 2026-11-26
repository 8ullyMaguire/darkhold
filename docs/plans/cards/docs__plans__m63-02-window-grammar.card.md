# docs/plans/m63-02-window-grammar.md — plan card

Source: `docs/plans/m63-02-window-grammar.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

Item: `docs/requirements.csv` M63-02. Plan written before code, per the standing prompt.
**Why the parser is in `crates/domain`, not the route:** the CLI (`dh rec --updated '>7d'`,
| `crates/domain/src/window.rs` | new: `Bounds`, `parse(raw, now)`, `WindowError`, unit tests |
| `crates/domain/src/lib.rs` | `pub mod window;` |
| `crates/db/src/catalogue.rs` | `CatalogueQuery` gains `updated/refreshed/added: Option<Bounds>`; `search` adds the predicates |
| `crates/app/src/routes/catalogue.rs` | three new query params, parsed with the shared parser; a bad window is a 422 naming the field |
| `crates/db/tests/catalogue_window.rs` | new: partition, NULL exclusion, three timestamp shapes |
   workspace currently fails on three pre-existing `work_ratings.rs` warnings; the item's own files
   commits). Recorded in `docs/plans/deploy-lineage-decision.md`, waiting on the operator.
history again. `docs/plans/deploy-lineage-decision.md` is resolved by that, and the deploy ran from

## Copy-pasteable code / commands

7d 36h 3w 6m 2y          within the last N (upper bound unbounded)
>7d >1y                  older than N            (upper bound now-N, half-open)
-30d..-7d                between 30 and 7 days ago
2026-10-01..2026-11-01   absolute range, half-open [start, end)
..-7d                    everything older than a week ago
@2026-11-01              that calendar day
today yesterday this-week last-week this-month last-month this-year last-year
any                      no constraint (the default)

-- SQLite
datetime(l.library_items_col) >= datetime(?1) AND datetime(l.col) < datetime(?2)
-- PostgreSQL
(l.col)::timestamptz >= (?1::timestamptz) AND (l.col)::timestamptz < (?2::timestamptz)

-- the column
CASE WHEN col ~ '(Z|[+-][0-9]{2}:[0-9]{2})$' THEN (col)::timestamptz
     ELSE ((col)::timestamp AT TIME ZONE 'UTC') END
-- the bound (always formatted naive-UTC by Bounds::from_sql)
((?::timestamp) AT TIME ZONE 'UTC')

## Verification command and expected output

   `cargo test -p darkhold-domain window` → `running N tests … test result: ok.`
   `cargo test -p darkhold-db --test catalogue_window` → ok; and the same file with
   `cargo test -p darkhold-app --test catalogue_window_route` → ok.
   a. Flip `<` to `<=` in the upper bound → the partition test must fail with a duplicate row.
   b. Drop the `datetime()` normalisation → the mixed-format test must fail.
   `cargo test -p darkhold-db --test catalogue_window` after each: FAILED, then green when reverted.
5. **Gates.** `cargo test -p darkhold-domain -p darkhold-db -p darkhold-app --lib` and
| grammar | `cargo test -p darkhold-domain --lib window` | 25 passed |
| predicates, both engines | `cargo test -p darkhold-db --test catalogue_window` | 4 passed (SQLite) / 4 passed (PG, session set to `Europe/Madrid`) |
| wiring | `cargo test -p darkhold-app --test catalogue_window_route` | 2 passed |
| lib gates | `cargo test -p darkhold-domain -p darkhold-db -p darkhold-app --lib` | 236 / 157 / 784 passed |
| mutation | test that failed |

## RED/GREEN evidence

   `cargo test -p darkhold-db --test catalogue_window` after each: FAILED, then green when reverted.
| grammar | `cargo test -p darkhold-domain --lib window` | 25 passed |
| predicates, both engines | `cargo test -p darkhold-db --test catalogue_window` | 4 passed (SQLite) / 4 passed (PG, session set to `Europe/Madrid`) |
| wiring | `cargo test -p darkhold-app --test catalogue_window_route` | 2 passed |
| lib gates | `cargo test -p darkhold-domain -p darkhold-db -p darkhold-app --lib` | 236 / 157 / 784 passed |
Mutations, all reverted green afterwards:
| mutation | test that failed |
