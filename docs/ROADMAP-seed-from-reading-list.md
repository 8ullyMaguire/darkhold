# ROADMAP — seed darkhold from the rated reading list

Status: proposed 2026-11-11, **superseded in part 2026-11-16** by `.hermes/plans/2026-11-16_203519-fanfiction-quality-import-and-seed.md`. Re-censused **2026-11-17**: Phase 2 shipped as `darkhold seed read-list --file` (row 407); Phase 4 remains open. The parser is the Rust command itself; `scripts/parse_read_list.py` remains the measurement tool only. The ranked idea bank is `docs/ideas/2026-11-17-reading-list-and-catalogue-quality-ideas.md`. Owner: darkhold member agent. Ledger row: 407 (estate) / M45-74 (repo).
Source of truth for the list: `/personal/literature/fanfiction/read.txt` (on thinkcentre; 268 lines, 27 KB, last modified 2026-11-02).

## Why this exists

`taste_census.py` (estate, ledger 390) measured darkhold on 2026-11-11:

    accounts 20 · taste_profiles 2 · instance_theme 1 · rating 0 · work_ratings 0 · work_taste_vectors 0

Twenty accounts and not one rating. A taste graph with no ratings has nothing to fold, so every ranking surface on the member with the most traffic is
decoration. The operator has 268 rated fan-fiction entries in `read.txt` — a real, opinionated signal that can bootstrap the arena, the rating table, the
taste profiles and the instance theme in one pass, before a single new user arrives.

This roadmap is the honest version of that: it seeds **data**, through the **product's own write path**, and it says plainly that one person's list is a
prior and not a population.

## Non-goals

- Not a substitute for the rating UI. If a person cannot rate a work by hand, seeding is a museum of the operator's taste with no way in; ledger 406 stays
  open until a signed-in person can rate in under ten seconds.
- Not a bulk SQL insert. Rows written straight into `rating` would leave `work_taste_vectors`, `taste_profiles` and `instance_theme` disagreeing with the
  table, and no provenance to tell seeded rows from human ones. The seeder calls the same service function the route calls.
- Not an import of work *text*. Titles, authors, ids and ratings only. No files, no downloads, no scraping.
- Not the 1,000-user goal. 218 parsed entries (of 268 lines) from one account moves the count from 0 to 1 rater; the census must be re-run after and the number recorded as it is.

## Adopts

- M45-74 — seed the instance from the rated reading list. This document is the plan for that row: the phases below are
  the steps, and the row is where its state lives. (Registered 2026-11-12: the roadmap existed and the ledger did not
  have a row for it, so the idea catalogue's coverage gate could not tell a live proposal from a forgotten one.)

## What the source actually looks like

    $ head -4 /personal/literature/fanfiction/read.txt
                                        MayorHaggar
    11871 9    Chastity                                                     MayorHaggar
    1171  9    A Precise Note [MHA | Izuku-Centric]                           Sir Lucifer Morningstar
    150   9    A Precise Note                                                 Sir Lucifer Morningstar
                                                                              (Neckbeard_Satan)
    1862  9    Rogue Knight                                                 Illuviar

    $ wc -l /personal/literature/fanfiction/read.txt
    268 /personal/literature/fanfiction/read.txt

Four columns, space-padded and occasionally wrapped (author names run onto the next line; blank-ish lines carry only an author). The first column is the
site's work id, the second the rating on a **0–10 scale**, then title and author. Ratings seen: 9, 8, 7 … so the list is dense at the top.

**Parsing rule** (do not improvise a fixed-width split — the columns shift with title length):

    ^(\d+)\s+(\d{1,2})\s+(\S.*?)\s{2,}(\S.*)$

**Measured 2026-11-11** with that rule (`/tmp/parse_read_list.py`, kept beside this doc's plan):

    lines            268
    matched          218
    wrapped_title      3   (reported, never guessed)
    unmatched         47
    distinct ids     218   (no duplicate work ids)
    rating spread  {10:9, 9:7, 8:36, 7:15, 6:50, 5:18, 4:52, 3:7, 2:21, 1:3}   (sums to 218)

Measured again 2026-11-16 by the sidecar plan. The unmatched 47 are the command line, the header, blank lines and rows the parser cannot classify.
Three rows have a **title wrapped onto the next line**, so the author column falls away from them:

    line  69: 2398  7    Shut up and Sith: or how I learned to keep calm and UNLIMIT
    line  70:            Gamer Story

So the seeder needs a **wrap-recovery step before parsing**: if a line does not match and the *next* line starts with ≥11 spaces and contains no leading
integer id, join it to the previous line with a single space and retry. Recoverable rows must be counted separately from truly unmatched ones, because the
three wrapped rows are named in the sidecar with `status: wrapped_title` and are never turned into guessed ratings.

The list's own provenance, from line 1 — record it, so the data can be reproduced:

    calibredb list --search '#last_read:True' --fields='*rating,title,author' …

In Calibre `*rating` is a **0–10** field (stars × 2), which matches the observed spread and settles the scale question in Phase 2.

## Phase 0 — measure before writing (no code)

Run these and paste the output into the PR body:

    ssh thinkcentre 'cd /personal/documents/code/rust/darkhold'
    # 1. Is there a write path at all? CORRECTED: yes — the route has existed since row 406.
    rg -n "route\(.*rate|rate_work|submit_rating|create_rating|upsert_rating" -g '*.rs' -g '!target' crates/app/src
    # Expected today: `PUT /api/v1/works/{id}/rating` -> routes/reading.rs::upsert_rating -> reading::upsert_rating.
    # The earlier "nothing — only reads" expectation was a false negative from a single-line grep.
    # 2. What scale does `rating` use? (column type, check constraint, existing rows)
    rg -n "CREATE TABLE.*rating|CHECK.*rating|rating .*(SMALLINT|INT)" -g '*.sql' crates/ | head
    # 3. Which table holds external ids, and does it know AO3/FFN ids?
    rg -l "work_external|external_id|source_id" -g '*.sql' crates/ | head
    # 4. Who owns the seed account, and what is its pseud?
    psql -d darkhold -Atc "select id, name from accounts order by id limit 5"

**Corrected 2026-11-12 — the measurement above was a false negative.** The route exists and always did:
`crates/app/src/routes/reading.rs:59` registers `.route("/works/{id}/rating", get(get_rating).put(upsert_rating).delete(delete_rating))`, the client
has `upsertRating` (`frontend/src/lib/api.ts:1113`), `Rating.svelte` drives it, `routes/WorkPage.svelte:647` mounts it, and
`crates/app/tests/milestone_4.rs:435` PUTs it over HTTP. The scan missed it because the pattern `.route(.*rate` cannot match a router that puts the
path on the line after `.route(`, and because the handler is named `upsert_rating` in a file named `reading.rs`, so the noun search missed it too.

Three things about that route shape the seeder: it is **`RequirePseud`** (a rating belongs to a pseud, so `--account` alone is not enough), it refuses
`stars` outside **1..=5** (`Calibre *rating` is 0–10, and a Calibre 0 means never rated — skip it, do not store 1), and it supports optimistic
concurrency via `expected_version` (a seeder should not send it).

**Ledger 406 is therefore closed, not a prerequisite**: the write path exists and the zero ratings were a population fact. Phase 2 below is the whole
job; the estate brief for it is `~/secondbrain/10-Projects/briefs/darkhold-seed-reading-list.md`.

## Phase 2 — the seeder, dry-run first

New CLI subcommand, following the member's existing CLI conventions (`docs/cli-exit-codes.md` for the exit-code vocabulary):

    python3 scripts/parse_read_list.py /personal/literature/fanfiction/read.txt   # measure only, no artifact
    darkhold seed read-list --file /personal/literature/fanfiction/read.txt                          # dry-run, writes nothing
    darkhold seed read-list --file /personal/literature/fanfiction/read.txt --apply --account <id> --pseud <id>

The Rust command owns text parsing and every database write, and the provenance stamp is a nullable `rating.source` written by `reading::upsert_rating`.
Dry-run is the default, `--apply` is required to write, and `--file` reads the export directly: no second parser and no sidecar.
Match order, first hit wins, is recorded per row: external id + source (materialised `library_items`, `source_key`/`source_work_key`); then normalised
title + author (that table's `title`/`author_text`, case- and punctuation-folded); then report as unmatched. The old sentence "matching is exact
normalised title only" was false: `works` has no author column, but `library_items` does, and a title alone is not identity. A title+author key with
more than one distinct work id is `ambiguous`, never chosen.
Scale is settled in the producer: Calibre's 0–10 rating becomes 1–5 stars (half up), and Calibre 0 is `unmatched` rather than a 1-star rating.

## Phase 3 — apply, through the service

For each matched row with no existing rating from the seed account:

- call `darkhold_db::reading::upsert_rating` — the same function `PUT /works/{id}/rating` calls — with `account`, `pseud`, `work` and 1–5 stars;
  a rating belongs to a pseud, not merely an account, and `is_public = false` keeps one person's reading history private;
- stamp provenance in the row itself: migration `0216_rating_source.sql` adds nullable `rating.source`, and `reading::upsert_rating` writes
  `'seed.read_list'`. The old sentence "there is deliberately no `rating.source` column" was false after row 407 — the audit sidecar needed a second
  write and could drift from the row it described; a nullable column is the one fact in one place;
- idempotent: a second `--apply` with the same export writes zero rows and says so. A seeded row whose stars changed in the export is refreshed
  through the same upsert; a human rating (NULL source) is never rewritten.

Never `INSERT INTO rating` directly. If the service cannot be reached in-process, the seeder fails loudly — a seeder that bypasses the route is a seeder
that certifies a path the product does not have.

## Phase 4 — rebuild what the ratings feed

After apply, run the member's own rebuild (the code the nightly job runs, not a hand-written query):

- `work_taste_vectors` — per-work vectors from the new ratings;
- `taste_profiles` — the operator's profile, now with 200+ signals instead of 0;
- `instance_theme` — the instance vector, so the landing page has a prior to show.

Record the row counts before and after in the PR body.

## Phase 5 — the verification that makes this a closure and not an activity

    # after --apply
    psql -d darkhold -Atc "select count(*) from rating"                       -- expect ≈ matched rows (not 0)
    jq -r '.work_id' audit.jsonl | sort -u                              -- the sidecar is the provenance
    psql -d darkhold -Atc "select count(*) from work_taste_vectors"           -- expect > 0
    psql -d darkhold -Atc "select count(*) from taste_profiles where account_id=<operator>"  -- expect 1, signal count > 150
    python3 /personal/documents/code/estate/scripts/taste_census.py           -- darkhold line must stop reading 0

Plus a test: seed a 3-row fixture twice, assert 3 rows and no duplicates the second time (`applied 0 skipped_existing 3`). A parser test over the
real file pins **218 matched, 47 unmatched, 3 wrapped_title, 268 lines**. The acceptance gate is a ranking intersection: after seeding, at least one of
the operator's top-10 personal recommendations must be a work in the audit sidecar. Zero means the seed did not reach a ranking and a wiring row opens;
it does not mean a new ranking crate gets built.

## Phase 6 — what this does and does not buy

- **Buys**: 20 accounts stop being an empty taste graph; the instance theme has a real prior; the arena and the summary endpoints have something to weight;
  ledger 390's cross-member distance has a second population's worth of signal (suki's 20,808 votes and darkhold's ~200 seeded ratings).
- **Does not buy**: users. The census after this will read "1 rater, 268 entries" and that must be recorded as the number, not rounded up to a population.
- **Privacy**: the list is the operator's own reading history. It is attributed to the operator's account with provenance, so it can be deleted in one
  statement if the decision is reversed.

## Order

1. Sidecar producer + parser tests (no database writes; safe to land alone).
2. CLI shape + exact-title matcher + dry-run report (no writes).
3. Apply through `upsert_rating` + audit sidecar; prove idempotence on scratch SQLite.
4. Copy-of-live apply with the fixed accept rule (≥180 exact, ≤20 ambiguous, ≥180 applied), then live apply.
5. Ranking intersection gate; refresh wiring for crawled metadata; corrected numbers pasted into the PR.
6. Estate row 407 closes only when the owner accepts the census numbers.
