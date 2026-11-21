# Spec — TA2: daily fanfiction aggregation, Pawchive chapter grouping and unbounded taste body archive

Status: active 2026-11-20. Supersedes TA1's 20,000-work body ceiling and 100-work pilot gate.

## Problem

TA1 shipped the local machinery, but the live instance still runs an older binary and its syndication config does not walk the operator's chosen Pawchive author graph. Pawchive imports currently treat chapter submissions as separate library works, the freshness window is wrong, other archives are not checked daily, bodies are not archived for taste-matching works, and the 20k ceiling contradicts the actual product goal.

## Goals

1. Deploy the current TA1+TA2 binary to thinkcentre.
2. Walk a BFS of depth 3 from one Pawchive seed: `https://pawchive.pw/patreon/user/13212571/recommended`.
3. Merge Pawchive chapter submissions into one Darkhold library work, preserving each source URL and provenance.
4. For Pawchive, admit only updates older than four months (`older_than = 120d`).
5. For AO3, QQ, SV, SB and the other supported archives, check daily and do not apply the Pawchive staleness window.
6. Archive bodies, not only metadata, for works whose tags contain at least one liked term and no excluded term.
7. Remove the global 20,000-work body ceiling; an optional per-run `--limit` may remain as an operational brake.
8. Expand adapter coverage, one fixture-backed source at a time, and record why uncovered sources are skipped.

## Non-goals

- No auth, cookies, paywall/private-work access, access-control bypass, proxy rotation, comment scraping or republishing.
- No new reader-facing schema blobs and no publication of the private preference file.
- No claim that a source with no adapter or no discovery shape is covered.
- No new crawler or second import pipeline; discovery must keep enqueuing the existing worker.

## Design

`source_sync` remains the only discovery path. A single `AuthorSeed` with `source_id = "pawchive"` starts a depth-3 BFS over Pawchive's `similar_creator_urls`. The existing `SafeFetcher`, robots posture, idempotency key and per-job `body_policy` are unchanged.

Pawchive chapter grouping is decided before a library item is written: posts with the same canonical title and author collapse to one work; each chapter/post URL remains in that work's provenance and import units. Existing duplicate Pawchive library items are repaired by a bounded CLI pass, not by a destructive migration.

Bodies are stored through the existing import worker with `body_policy: store`. The selector is the private taste scorer, not a new taxonomy: a candidate must match a positive term and must not match a hard exclusion. `dh archive bodies` stays the single apply point; a scheduled wrapper may call it daily, but discovery itself remains `body_policy: skip`.

The 20k cap is removed from `config/darkhold-taste.local.json`, `LoadedPreferences` and `crates/cli/src/archive.rs`. There is no global shortfall target; reports state discovered, matched, queued, skipped-existing and failed counts.

Adapter expansion uses `crates/scrapers/sources.json` as the source of truth. A source moves from `adapter: null` to a named adapter only after fixture-backed `preview_from_html` and `units_from_html` tests exist.

## Adapter coverage disposition (TA2-8)

Added in TA2: `efpfanfic`, `mediaminer`, `whofic`, `adult-fanfiction` now use the existing eFiction adapter. They are fixture-proven with the family fixture and host-claim test; no new parser was created.

The remaining catalogued sources without an adapter are explicitly skipped, not forgotten:

- Site-wide robots or unreachable: `fanfiktion`, `honeyfeed`, `cfaarchive`, `bulbagarden`, `storiesofarda`.
- Metadata-only by catalogue policy: `animexx`, `audiofic`, `tthfanfic`.
- Custom markup needing a dedicated fixture set: `storysea`, `quotev`, `femslash`, `asianfanfics`, `fimfiction`, `lodginghouse`, `lcficmbs`, `ficwad`, `merengo`.
- XenForo family candidates needing their own forum fixtures: `thousandroads`, `gateworld`, `theforce`.

Each group has a written reason here, so the catalogue no longer presents an unimplemented source as if it were covered.

## Success criteria

- Live meta build hash matches the deployed TA2 commit.
- Syndication log shows daily passes; Pawchive BFS report has `discovered > 0` from the one seed at depth 3.
- A Pawchive work with several chapter submissions has one `library_items` row and multiple recorded source URLs/units.
- Pawchive report shows only works older than 120 days admitted.
- AO3/QQ/SV/SB reports pass daily without the 120-day filter.
- A body import is queued and stored only for candidates with a positive taste match and no excluded tag.
- `dh archive bodies --limit 25000` is not capped at 20,000.
- Every newly added adapter has a fixture test; the repository gate is green at the end.

AO3 refetch acceptance is coverage of source-derived `work_tags`, not the `works_tagged` counter alone. `dh catalogue --source ao3 --refetch` takes `--offset N` and excludes works that already have `work_tags.source = 'imported'`. If AO3 returns HTTP 429, the row is blocked; no auth, cookies, bypass, or unbounded retry is permitted.
