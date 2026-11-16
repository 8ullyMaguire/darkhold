# Amendment 71 — the syndicated trope frontier

Status: proposed, 2026-11-17. This is an amendment, not spec §71: `scripts/split-spec.py` owns §1–70 and reserves 62–70. It extends §55 (source adapters), §56 (imports) and §21 (reader contract) without renumbering anything.

## Problem

Darkhold already has 33 reviewed source platforms, eleven adapters, a declarative manifest, a source catalogue and a periodic `source_sync` worker. The worker only walks tag pages and one ScribbleHub listing. It has no answer to the owner's actual reading frontier: *system fics and dark MCs, including the authors that favourite authors recommend*. Pawchive has an adapter but is not in the catalogue, its adapter models one author as one work rather than one story, and `list_author_works` refuses the `/recommended` page the owner uses as a seed.

## Sources found

The search was run on 2026-11-17. A source is listed only when a public page ties it to the requested tropes; the existing catalogue remains the authority for robots and body policy.

| Source | Evidence | Use in this amendment |
|---|---|---|
| Pawchive | `https://pawchive.pw/patreon/user/26677387/recommended` — "Artists similar to Vandalvagabond"; 50+ public author links. Post API `https://pawchive.pw/api/v1/patreon/user/26677387/posts?limit=5&o=0` shows chapter posts such as "Sailing the Skies with my Endergirl 10". | Seed and BFS frontier. Public JSON for posts, public HTML for similar creators. Six-month freshness window. |
| Archive of Our Own | `https://archiveofourown.org/collections/Gamer_System_Fics_and_Original_Works`; `https://archiveofourown.org/tags/It's%20All%20The%20System's%20Fault%20(Scum%20Villain)/works`; `https://archiveofourown.org/users/The_Dark_Wolf_Shiro/...` with explicit "dark protagonist / evil mc" summaries. | Tag-page discovery through the existing AO3 adapter. Seven-day window. |
| SquidgeWorld, Ad Astra, Fanfiction.lol | Already in `crates/scrapers/sources.json` as AO3-software hosts with `tag_page` discovery; AO3 tag evidence transfers to their software family. | Tag-page discovery. Seven-day window. |
| FanFiction.net community "Evil Dark Main characters" | `https://www.fanfiction.net/community/Evil-Dark-Main-characters/132017` — 220 stories, explicit dark/evil MC focus. | Catalogue says `metadata_only`, host unreachable here: declared but never queued until the verdict changes. |
| Royal Road | `https://www.royalroad.com/fiction/64916/...` ("system apocalypse", "A Dark MC"). | Catalogue says `blocked`: metadata and note only, no unattended walk. |
| Wattpad | `https://www.wattpad.com/stories/systemfiction` — public `systemfiction` + `darkromance` tag page. | Catalogue entry needed; no adapter yet, so declared but never queued. |
| Quotev | `https://www.quotev.com/stories/c/Fanfiction/System` — public "System Fanfiction" browse. | Already catalogued, no adapter: declared but never queued. |
| ScribbleHub | Existing `listing` discovery; Royal Road-style system/dark web fiction is in family. | Existing listing walk; seven-day window. |

A source with no adapter or a blocked/metadata-only verdict is a **declaration**, not a promise. It appears in the catalogue with the reason; it queues nothing.

## Requirements

### R71.1 — per-source freshness config (check: config tests)

`SyndicationConfig` gains `sources: Vec<SourceSyncRule>` where each rule has `source_id`, `enabled`, `newer_than` and `older_than`. Durations accept the site's existing `Nh/Nd/Nw/Nm` grammar (hours, days, weeks = 7d, months = 30d) and are optional per side. A candidate is eligible when `published_at` (or the listing's updated time when published is absent) is strictly newer than `newer_than` and strictly older than `older_than`. Default for every source is `older_than: 7d`; Pawchive is `older_than: 6mo`. Per-source rules override defaults per field, not wholesale. Zero and negative windows are refused at config load rather than silently meaning "everything".

### R71.2 — author-frontier discovery (check: scraper + source_sync tests)

`Discovery` gains `AuthorFrontier { recommended_template }`. The catalogue's Pawchive entry uses `/patreon/user/{id}/recommended`. `SyndicationConfig` gains `author_seeds: Vec<AuthorSeed>` (`source_id`, `author_url`). One seed is shipped: Pawchive `26677387`. Breadth-first traversal visits a seed at depth 0 and expands its similar creators through depth 3, never revisiting an author id and never exceeding `max_authors_per_pass`. Depth is a safety bound, not an accuracy claim. A failed page is recorded on that node and does not kill the pass.

### R71.3 — story identity, not post identity (check: importer tests)

Pawchive posts are grouped into one work per `(author_id, normalised story title)`. The normaliser strips a trailing chapter number (`... 10`), a trailing `Chapter 10` and punctuation, case-folds, and collapses whitespace; it must not strip numbers that are part of the name (e.g. `Gachapunk 2076`). The work's `source_work_key` is `pawchive:{author_id}:{slug}`; each post becomes a unit keyed by the post id. Re-importing chapter 11 of the same story adds a unit to the existing work, not a second work. Voting/announcement posts with no prose remain skipped.

### R71.4 — metadata public, body TL2, no low-trust indication (check: HTTP tests on both engines)

Imported works carry `body_audience = 'trust_at_least:2'`. Anonymous and TL1 readers receive title, author, tags, word count and status from search/work listings. They do **not** receive body text, an excerpt, a chapter count, a "restricted/unlock/TL2" badge, or any count that reveals a withheld body exists. The withheld test asserts the difference between an imported TL2 work and an ordinary public work in the same payload, so an empty result cannot pass. Staff reviewing their own imported works remain entitled to read them.

### R71.5 — periodic scheduling (check: existing worker test plus new gate)

The existing `source_sync` worker and its interval-bucketed `enqueue_if_due` are reused; no second scheduler. The rule key is derived from the interval, with `max(1h)` floor. `enabled=false` and `enabled=true` with zero sources/seeds both queue nothing. A second pass over unchanged listings reports all work ids as already known; a late new post moves the stored unit set (the `DO NOTHING` mutant must die). The run report names discovered, queued, known and skipped per source.

## Non-goals

- No new story file format, no direct `INSERT INTO rating`, no new ranking crate.
- No login to Patreon, Wattpad or any paywalled body source; robots verdicts stay authoritative.
- No client-side crawl, no browser extension, no change to the reader's typography.
- No retroactive rewrite of already-imported works in this amendment; a repair migration is a later row.

## Decision record

- **Depth 3** means seed at depth 0 and three expansion hops. Recorded here because "depth 3" otherwise reads as four levels to some implementers.
- **Six months = 180 days** in config, matching the existing `Nm` grammar's 30-day month; the owner's prose says six months.
- **Freshness is a listing-side filter** before enqueueing an import; the import pipeline stays the only place that fetches and stores bodies.
- **No low-trust hint** is a negative feature: the test must assert absence in the rendered response, not just that the body route 404s.

## Completion gate

```text
cargo test -p darkhold-scrapers --lib pawchive_frontier
cargo test -p darkhold-app --test syndication_frontier
cargo test -p darkhold-app --test import_visibility
cargo test -p darkhold-db --test imported_story_identity_two_engines
bash scripts/gate.sh repo
```

Every command above must print a passing count, and the body-hiding assertion must fail when the `trust_at_least:2` audience is removed.
