# Amendment — one query for stale, popular, unrated works across sources

Status: **proposed 2026-11-24** · Owner: darkhold member agent · Ledger row: **M45-75**
Amends: `docs/spec/11-search-and-discovery.md` §11.2, §11.7 · Plan:
`docs/plans/stale-popular-unrated-search.md` · Roadmap:
`docs/ROADMAP-stale-popular-unrated-search.md`

## The request

> system progression fantasy (gamer, celestial, cyoa, ...) from any source
> (questionable questing, sufficient velocity, space battles, fanfiction.net,
> archiveofourown, ...) that hasn't updated for the last 4 months sorted by
> popularity and skipping the ones I've already rated

One expression, one endpoint, one ordered page. It is the shape of question the
spec's "one query language across every surface" (§11.1) exists to answer, and
today darkhold **cannot** answer it.

## What is possible today, measured

Probes against the deployed instance `127.0.0.1:8081` on 2026-11-24,
`GET /api/v1/search` (the AST door `dh q` uses), plus read-only SQL on the live
`darkhold` database. Commands and outputs are in the plan's step 0.

| Part of the question | Today | Evidence |
|---|---|---|
| "system progression fantasy (gamer, celestial, cyoa, …)" | **Partly.** `trope:system-grind` expands to `system/gamer/litrpg/stat-grinding`; `trope:dark-progression` includes `progression`. `celestial` and `cyoa` exist as tags, but no group names them, so they must be spelled by hand. | `crates/domain/src/tropes.rs`; live `tag:cyoa` 5 hits, `tag:celestial` 1 hit, `tag:progression` 1 hit, `trope:system-grind` 5 hits |
| "from any source (qq, sv, sb, ffnet, ao3, …)" | **No.** `QueryField` has no source field. `source:qq` parses as free text and returns **200 with 0 hits**, not a refusal. `dh rec --source` takes one key and belongs to a different surface with no tag/trope grammar. | live `?q=source:qq` → 200, `items: []`; `crates/domain/src/query.rs` field enum; `crates/cli/src/rec.rs` |
| "hasn't updated for the last 4 months" | **No, on the clock that matters.** `updated:` reads `works.updated_at`, which is the import/write clock: **0 of 58,042** live works are older than 120 days. The author's clock is `library_items.source_updated_at`: **86,541 of 90,662** rows are older than 120 days. | live SQL; `crates/domain/src/query_sql.rs::date_column` |
| "sorted by popularity" | **No.** The search route accepts and echoes `sort=popularity`, `sort=likes`, `sort=kudos`, but `render_statement` recognises only `newest` and `title`; every other value falls back to term-count score. A live 12-hit probe returned an identical order for all five spellings. | `crates/db/src/search/ast_search.rs` `order_by` match; live probe |
| "skipping the ones I've already rated" | **No.** No viewer-relative predicate exists in the grammar or the search SQL. `rating` (per pseud, stars, `deleted_at`) is the table that holds the fact. | `crates/domain/src/query.rs` `QueryAst`; `migrations/sqlite/0004_reading.sql`; live probe with `exclude_rated=1` ignored |
| terminal invocation | **No.** `dh q` has no source, sort or exclude-rated flag; it has no viewer credential path either. | `crates/cli/src/q.rs::QArgs` |

Root cause in one sentence: the query language, the catalogue surface and the
recommendation CLI each hold one third of the vocabulary, and none of them
holds all of it.

## Goals

1. One `GET /api/v1/search?q=…&sort=…` request can express all five clauses,
   with the same meaning on the web search box, saved views, feeds and `dh q`.
2. Unknown fields and unknown sort names are **refused by name** (422) rather
   than answered with an empty page or a silently ignored order. The `source:qq`
   empty-200 above is the failure mode being designed against.
3. The "not updated" clause uses the **source's** clock; `updated:` keeps its
   existing meaning (our clock), so no existing query changes meaning.
4. "Already rated" is scoped to the calling viewer; an anonymous caller gets a
   named 401 rather than a page that silently includes everything.
5. The trope names the reader is likely to type (`system progression fantasy`,
   `gamer`, `celestial`, `cyoa`) resolve through one group without exceeding the
   24-term budget, including five sources and the other predicates.

## Non-goals

- **No new data source or scraper.** Source keys and `source_updated_at` already
  exist in `library_items`; the five named archives are already ingested. An
  un-ingested source stays absent and is reported as such.
- **No semantic/vector search, no `me:read`, `me:dnf`, `me:bookmarked`.** Only
  `me:rated` is in scope; the wider `Me(MeField)` design stays in
  `docs/plans/advanced-search-as-built.md`.
- **No change to `/api/v1/catalogue`'s own `source=` / `updated=` grammar.** It
  is a different surface with a single source key and stays as it is. This
  amendment adds the vocabulary to the query language, not a second spelling.
- **No redefinition of `updated:`.** `updated:<=-4m` on `works.updated_at`
  remains "we have not touched this record for four months" and will keep
  returning nothing on this corpus.
- **No frontend redesign.** The search page may surface the new fields later;
  this row is the language, store, route and CLI.
- **No full-workspace gate.** Targeted tests on SQLite and PostgreSQL only, per
  the standing rule.

## The canonical query

```
(trope:system-progression) \
AND (source:qq OR source:sv OR source:sb OR source:ffnet OR source:ao3) \
AND source_updated:<=-4m \
AND NOT me:rated
```

with `sort=popularity` as a separate parameter (an ordering is not a
predicate, §11.7). The equivalent terminal command is

```
dh q '(trope:system-progression) AND (source:qq OR source:sv OR source:sb OR source:ffnet OR source:ao3) AND source_updated:<=-4m AND NOT me:rated' \
  --sort popularity --exclude-rated --jsonl --fields title,source,updated
```

`--exclude-rated` appends `AND NOT me:rated` so the saved expression remains
the shareable object; the flag is sugar, not a second grammar. (If both are
given, append once.)

### Vocabulary decisions

- `source:` is a works-surface field; a value is the stored `source_key`
  (`sb`, `sv`, `qq`, `archive`, `pawchive`, `ao3`, `ffnet`, …). Five sources are
  five `OR`-ed fielded terms, not a comma list: a comma would need a second
  parser for a list the grammar already expresses.
- `source_updated:` is a **date-comparable** works field using the
  `window::instant` age grammar (`-4m` = 120 days, `-120d` equivalent), rendered
  as a plain text range against `library_items.source_updated_at`. A work with
  no source row never matches; it is not "stale".
- `me:rated` is a viewer-relative leaf, allowed as both a positive and a
  negated term. It compiles to
  `NOT EXISTS (SELECT 1 FROM rating r WHERE r.work_id = works.id AND r.account_id = ? AND r.deleted_at IS NULL)`
  when negated. The account id is already threaded through search for
  visibility and blocks; this is a third bind of the same value.
- `sort=popularity` orders by the maximum `library_items.likes` across the
  work's source rows. Aliases `likes`, `kudos`, `aggregate` map to the same
  order; `updated`/`newest` and `title` keep their current meanings. Anything
  else is a 422 that names the sort word and the available vocabulary. (A
  stored default from an older build already uses an accepted word; unknown
  stored values are refused by name, not silently downgraded.)
- The new trope group is `system-progression`, members
  `system, gamer, litrpg, stat-grinding, progression, celestial, cyoa`. Seven
  members cost seven terms; with five sources, one date and one viewer term the
  query costs 14 of 24.

## Success criteria (fix the rule before measuring)

1. The canonical query returns at least one hit on the live instance, and every
   returned row carries a source key in the five named keys and a
   `source_updated_at` older than `2026-07-26` (120 days before 2026-11-24).
2. The same query without `AND NOT me:rated` returns a strict superset for a
   viewer who has rated at least one of those works; for a viewer with no
   ratings the two sets are equal. Asserted in a two-engine test, not by eye.
3. `?q=source:qq` returns **422** naming `source` as a works field or 200 with
   hits — never a silent empty page. `?q=source:qq OR source:sv` returns rows
   from both archives.
4. `?sort=popularity` orders by descending likes; `?sort=newest` orders by our
   update clock; the two lists differ on a fixture built for that purpose. A
   mutant that removes the ORDER BY branch reddens the test.
5. `?q=me:rated` anonymous returns 401 naming the credential; with a session it
   returns only that viewer's rated works.
6. Both store tests pass on SQLite and PostgreSQL (`DARKHOLD_TEST_PG_URL`
   pointing at `lh-pg-test`), and `route_inventory` stays green.

## Rejected alternatives

| Alternative | Why it lost |
|---|---|
| Keep using `works.updated_at` for "not updated" | Measured 0/58,042 stale rows; it answers a question about our import job, not the author. |
| Compose `dh rec --source KEY --updated 4m --sort likes` five times and merge | `dh rec` has no tag/trope field, takes one source per call, and `--exclude-read` is reading history, not ratings. Five calls also give five different orderings and no shared limit. |
| Add a `not_rated=1` query parameter only | It would not be shareable in the query string, would need a second spelling in saved views, and would not compose with `NOT`. |
| Post-filter search results in the route | A page-sized post-filter cannot produce a page-sized result: the works it drops are never replaced, so "20 hits" means fewer than 20 rows. |
| Reuse `updated:` with a new meaning | Silently changes every saved query and feed that already uses it. |
| Add source columns to `works` | Duplicates provenance already stored on `library_items`; the two would drift, and migration 0132's timestamp contract would need repeating. |


## Follow-up 2026-11-25 — the hit must carry its own source fact

The amendment's own canonical command asks for
`dh q … --fields title,source,updated`, but `SearchResult` carries neither a
source key nor the source clock and `dh q` refuses both columns. A documented
query that cannot print its own answer is an unfinished surface, so three rows
are added here before the next pass.

- **M45-75.6** — a works search hit carries `source_key` and
  `source_updated_at` from the source row that supplied the popularity figure:
  the one with the highest `library_items.likes`, tie-broken by `source_key`.
  A work with no source row carries empty strings, never a fabricated date.
  `dh q … --jsonl --fields title,source,updated` must print those keys; the
  table's `--cols` vocabulary gains the same two names.
- **M45-75.7** — the ordering proof must be able to fail. The fixture gives the
  two works different `updated_at` clocks, and asserts that `sort=popularity`
  and `sort=newest` name different first hits. Removing the popularity `ORDER
  BY` arm must redden that assertion before the row closes.
- **M45-75.8** — the source predicates are proved against the live planner,
  not by assumption: `explain` must show the existing `library_items_work`
  index being used for the correlated `EXISTS`. No new index is created unless
  the plan shows a scan of 90k rows.

Non-goals unchanged: no source-key list grammar, no change to `/catalogue`, no
wider `me:` vocabulary, no visibility change to manufacture live hits.
