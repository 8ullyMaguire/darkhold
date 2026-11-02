## 2026-11-02 — M47-08: private search history (audit item 31)

**Requirement:** `M47-08` · **Tests:** 7 db + 6 route (both engines) + 6 frontend

The audit filed item 31 under "none needs a migration — all render work over columns that exist".
`grep -rn "search_history\|SearchHistory" crates/` returned nothing and no table held a query
string, so the one thing it needed was the one thing described as present.

```
POST-free surface:  GET /api/v1/search           records, after the search succeeded
                    GET /api/v1/search/history?limit=10   this pseud's last N
                    DELETE /api/v1/search/history         clears, returns the count
table:              search_history (0139, both dialects)
setting:            search.record_history (default on)
```

**Four decisions, each with the alternative it beat.** `query_key` is a stored column because
SQLite's `lower()` folds ASCII only, so an expression index would make `Éowyn` one row on
PostgreSQL and two on SQLite — the two engines disagreeing about the length of a history. No
`account_id`, because §51 erases an account by rekeying its pseud and an account column would hand
that history to the account that used to own it. A rejected query is not recorded, because the
write sits after `map_err(search_failure)?`. And `search.record_history = false` stops the writing
without hiding what is stored: the first version returned an empty list, which showed "no history"
while the rows were still on disk.

**Two engine bugs the gates caught, not a reading.** PostgreSQL rejected `result_count INTEGER`
read as `i64` ("mismatched types; Rust type `i64` (as SQL type `INT8`) is not compatible with SQL
type `INT4`") — now `BIGINT`. And `clear` used a bare `?`, which PostgreSQL reads as a literal
token and answers with "syntax error at end of input"; it goes through `db.sql` like its siblings.

**A pre-existing red gate, partly repaired.** `route_inventory::every_route_has_correct_audience`
was failing at HEAD on three routes nobody had tabled; the two finisher surfaces now declare
`MaybeSession` and the check is green. Its table-completeness half still lists ~20 untabled routes
in `translation`, `catalogue`, `supply`, `modules`, `welcome` and `rec_run` — deliberately not
half-done here, and named so the next pass starts from a count rather than a hunch.

```
cargo test -p darkhold-db  --test search_history          -> 7 passed (SQLite)
DARKHOLD_TEST_PG_URL=… cargo test -p darkhold-db --test search_history   -> 7 passed
cargo test -p darkhold-app --test m47_08_search_history   -> 6 passed (SQLite and PostgreSQL)
cd frontend && npx vitest run src/routes/Search.test.ts   -> 21 passed (6 new)
cd frontend && npx vitest run                             -> 591 passed
npx svelte-check --threshold error                        -> 0 errors
cargo test -p darkhold-domain                             -> 913 passed
```

**Red by removal:** dropping the `worth_recording` guard fails the keystroke test; dropping the
trim fails the cap test; dropping the route's recording gate fails the setting test; removing the
post-search refresh and the paused note fail their two frontend tests.

