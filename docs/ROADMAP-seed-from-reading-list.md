# ROADMAP — seed darkhold from the rated reading list

Status: proposed 2026-11-11. Owner: darkhold member agent. Ledger row: 407.
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

    lines          268
    matched        218
    unmatched       50   (19% — not "a handful", which is what this doc claimed before it was run)
    distinct ids   218   (no duplicate work ids)
    rating spread  {10:9, 9:7, 8:36, 7:15, 6:50, 5:18, 4:52, 3:7, 2:21, 1:3}   (sums to 218)

The unmatched 50 are: line 1 (the `calibredb` command itself, which is how the list was produced), line 2 (the header), and blank lines — plus rows
whose **title wrapped onto the next line**, so the author column falls away from them:

    line  69: 2398  7    Shut up and Sith: or how I learned to keep calm and UNLIMIT
    line  70:            Gamer Story

So the seeder needs a **wrap-recovery step before parsing**: if a line does not match and the *next* line starts with ≥11 spaces and contains no leading
integer id, join it to the previous line with a single space and retry. Recoverable rows must be counted separately from truly unmatched ones, because the
difference between "218 works" and "~235 works" is the size of the seed.

The list's own provenance, from line 1 — record it, so the data can be reproduced:

    calibredb list --search '#last_read:True' --fields='*rating,title,author' …

In Calibre `*rating` is a **0–10** field (stars × 2), which matches the observed spread and settles the scale question in Phase 2.

## Phase 0 — measure before writing (no code)

Run these and paste the output into the PR body:

    ssh thinkcentre 'cd /personal/documents/code/rust/darkhold'
    # 1. Is there a write path at all? (expected today: nothing — only reads)
    rg -n "route\(.*rate|rate_work|submit_rating|create_rating" -g '*.rs' -g '!target' crates/app/src
    # 2. What scale does `rating` use? (column type, check constraint, existing rows)
    rg -n "CREATE TABLE.*rating|CHECK.*rating|rating .*(SMALLINT|INT)" -g '*.sql' crates/ | head
    # 3. Which table holds external ids, and does it know AO3/FFN ids?
    rg -l "work_external|external_id|source_id" -g '*.sql' crates/ | head
    # 4. Who owns the seed account, and what is its pseud?
    psql -d darkhold -Atc "select id, name from accounts order by id limit 5"

Measured 2026-11-11 for step 1: no rating write route exists in `crates/app/src` (the only hit is a comment in `inventory.rs` about the M16-08 gallery,
where install/uninstall/update/rate are `Authenticated`). **Phase 1 therefore depends on ledger 406**: build the write path first, or the seeder has nothing
honest to call.

## Phase 1 — the write path (ledger 406, prerequisite)

Deliverable: `POST /works/{id}/rate` (form field `value`), authenticated, which

1. rejects a value outside the works scale, with a typed error the template can render;
2. inserts or updates one `rating` row per `(account_id, work_id)`, `ON CONFLICT … DO UPDATE`, stamping `updated_at`;
3. returns the trust-weighted summary for that work, so the caller sees the effect (this is what makes the gate measurable);
4. is covered by a test that rates twice and asserts one row and a changed value.

Gate for 406: a signed-in person rates a work in under ten seconds and the row lands in `rating` in the same request. Then and only then does Phase 2 run.

## Phase 2 — the seeder, dry-run first

New CLI subcommand, following the member's existing CLI conventions (`docs/cli-exit-codes.md`):

    darkhold seed read-list --file /personal/literature/fanfiction/read.txt --dry-run
    darkhold seed read-list --file … --apply --account <operator-id> --scale-divisor 2

Behaviour with `--dry-run` (default): parse, match, print a table and an unmatched report, touch no table, exit 0. Exit codes: `0` clean, `2` file
unreadable, `3` some rows unmatched (with the count on stderr), `4` `--apply` without `--account`.

Matching order, first hit wins, and the chosen strategy is recorded per row:

1. **external id + source** — if the works table stores an AO3/FFN id, exact match (the strong path; 268 rows should mostly land here);
2. **normalised title + author** — lowercased, punctuation stripped, whitespace collapsed, author compared as a prefix (accounts are often
   `(Neckbeard_Satan)` style suffixes);
3. **unmatched** — reported, never invented.

Scale: the list is 0–10, darkhold's own scale is whatever Phase 0 step 2 prints. `--scale-divisor` makes the mapping explicit and testable (0–10 → 0–5 at
divisor 2, rounding half up). If darkhold's scale is already 0–10, pass 1.

## Phase 3 — apply, through the service

For each matched row with no existing rating from the seed account:

- call the same service function `POST /works/{id}/rate` calls, with `account = operator`, `value = round(list_rating / divisor)`;
- write `taste_provenance` with `source = 'seed.read_list'`, the file's mtime, and the source row (`id`, rating) so the row can be traced back and
  revoked;
- idempotent: a second `--apply` writes zero rows and says so.

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
    psql -d darkhold -Atc "select count(*) from rating where source='seed.read_list'"  -- expect the same number
    psql -d darkhold -Atc "select count(*) from work_taste_vectors"           -- expect > 0
    psql -d darkhold -Atc "select count(*) from taste_profiles where account_id=<operator>"  -- expect 1, signal count > 150
    python3 /personal/documents/code/estate/scripts/taste_census.py           -- darkhold line must stop reading 0

Plus a test: seed a 3-row fixture twice, assert 3 rows and no duplicates; seed a row whose title matches nothing, assert exit code 3 and an unmatched report. And a parser test over the real file: **218 matched, 50 unmatched, 218 distinct ids** — a regression changes those three numbers.

## Phase 6 — what this does and does not buy

- **Buys**: 20 accounts stop being an empty taste graph; the instance theme has a real prior; the arena and the summary endpoints have something to weight;
  ledger 390's cross-member distance has a second population's worth of signal (suki's 20,808 votes and darkhold's ~200 seeded ratings).
- **Does not buy**: users. The census after this will read "1 rater, 268 entries" and that must be recorded as the number, not rounded up to a population.
- **Privacy**: the list is the operator's own reading history. It is attributed to the operator's account with provenance, so it can be deleted in one
  statement if the decision is reversed.

## Order

1. Ledger 406 write path + test (blocks everything below).
2. Phase 2 parser + dry-run + unmatched report (no writes; safe to land alone).
3. Phase 3 apply on a copy of the database, then on live.
4. Phase 4 rebuild, Phase 5 numbers pasted into the PR.
5. Re-run `taste_census.py`; update ledger 390 with the new counts; ledger 407 closes on the pasted numbers.
