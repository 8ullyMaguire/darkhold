# docs/ROADMAP-seed-from-reading-list.md — plan card

Source: `docs/ROADMAP-seed-from-reading-list.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

Status: proposed 2026-11-11, **superseded in part 2026-11-16** by `.hermes/plans/2026-11-16_203519-fanfiction-quality-import-and-seed.md`. Re-censused **2026-11-17**: Phase 2 shipped as `darkhold seed read-list --file` (row 407); Phase 4 remains open. The parser is the Rust command itself; `scripts/parse_read_list.py` remains the measurement tool only. The ranked idea bank is `docs/ideas/2026-11-17-reading-list-and-catalogue-quality-ideas.md`. Owner: darkhold member agent. Ledger row: 407 (estate) / M45-74 (repo).
    rg -n "route\(.*rate|rate_work|submit_rating|create_rating|upsert_rating" -g '*.rs' -g '!target' crates/app/src
    # Expected today: `PUT /api/v1/works/{id}/rating` -> routes/reading.rs::upsert_rating -> reading::upsert_rating.
    rg -n "CREATE TABLE.*rating|CHECK.*rating|rating .*(SMALLINT|INT)" -g '*.sql' crates/ | head
    rg -l "work_external|external_id|source_id" -g '*.sql' crates/ | head
`crates/app/src/routes/reading.rs:59` registers `.route("/works/{id}/rating", get(get_rating).put(upsert_rating).delete(delete_rating))`, the client
has `upsertRating` (`frontend/src/lib/api.ts:1113`), `Rating.svelte` drives it, `routes/WorkPage.svelte:647` mounts it, and
`crates/app/tests/milestone_4.rs:435` PUTs it over HTTP. The scan missed it because the pattern `.route(.*rate` cannot match a router that puts the
path on the line after `.route(`, and because the handler is named `upsert_rating` in a file named `reading.rs`, so the noun search missed it too.
New CLI subcommand, following the member's existing CLI conventions (`docs/cli-exit-codes.md` for the exit-code vocabulary):
    python3 scripts/parse_read_list.py /personal/literature/fanfiction/read.txt   # measure only, no artifact
- stamp provenance in the row itself: migration `0216_rating_source.sql` adds nullable `rating.source`, and `reading::upsert_rating` writes

## Copy-pasteable code / commands

No fenced command block was recorded in the source document.

## Verification command and expected output

taste profiles and the instance theme in one pass, before a single new user arrives.
  have a row for it, so the idea catalogue's coverage gate could not tell a live proposal from a forgotten one.)
    # Expected today: `PUT /api/v1/works/{id}/rating` -> routes/reading.rs::upsert_rating -> reading::upsert_rating.
New CLI subcommand, following the member's existing CLI conventions (`docs/cli-exit-codes.md` for the exit-code vocabulary):
Plus a test: seed a 3-row fixture twice, assert 3 rows and no duplicates the second time (`applied 0 skipped_existing 3`). A parser test over the
real file pins **218 matched, 47 unmatched, 3 wrapped_title, 268 lines**. The acceptance gate is a ranking intersection: after seeding, at least one of
5. Ranking intersection gate; refresh wiring for crawled metadata; corrected numbers pasted into the PR.

## RED/GREEN evidence

No RED/GREEN evidence was recorded in the source document.
