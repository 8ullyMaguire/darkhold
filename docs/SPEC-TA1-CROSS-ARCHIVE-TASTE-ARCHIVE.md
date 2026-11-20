# Spec — cross-archive metadata aggregation and private taste body archive (TA1)

Status: planned 2026-11-20. Implementer: this repository's `master` lineage.

## Problem

The operator wants Darkhold to be the place where fanfiction is catalogued, not a separate AO3 fetcher. Darkhold already knows many archives and has a safe import pipeline, but it does not yet aggregate metadata broadly, score it against a private taste profile, or hold a bounded body set for the works the operator is most likely to enjoy.

## Goals

1. Aggregate public metadata for every work discoverable from sources declared in `crates/scrapers/sources.json` and supported by an adapter in `crates/scrapers/src/sites/mod.rs`.
2. Rank those works against one private preference file using only source metadata and tags.
3. Store bodies for at most 20,000 public, robots-allowed, high-confidence works.
4. Report shortfall instead of padding when fewer than 20,000 high-confidence works exist.
5. Keep all taste data and stored bodies private; do not add reader-facing schema blobs.

## Non-goals

- No new crawler, recommender, source catalogue format or blob store.
- No auth, cookies, paywalled/private works, access-control bypass, proxy rotation, comment scraping or republishing.
- No reader-visible publication of private preferences or imported bodies.
- No claim that unsupported or robots-blocked sites are covered.
- No attempt to infer new taste from bookmarks in this slice.

## Design

Discovery stays in `crates/app/src/source_sync.rs`: it walks tag pages/listings/author frontiers with `SafeFetcher` and enqueues metadata-only imports. `crates/app/src/imports.rs` gains a per-job `body_policy`: `skip` for discovery, `store` only for selected works, `inherit` otherwise. The import pipeline remains the only writer of `library_items`, tags and blobs.

Source tags are preserved in `library_items.provenance_json`, because metadata-only imports leave `work_id` NULL and cannot rely on taxonomy joins. `crates/taste/src/curation.rs` is a pure scorer: positive terms add weight, hard exclusions remove a candidate, and every selected work carries a short reason string. `crates/db/src/taste_curation.rs` reads candidates on both SQLite and PostgreSQL. `dh archive rank` prints the ranking; `dh archive bodies` enqueues capped body-store jobs through the existing queue.

The private preferences file is `config/darkhold-taste.local.json`, mode 0600 and gitignored. It contains the operator's explicit terms, expanded from the AO3 searches the operator supplied: positives cover dark/grey/evil/villain OP protagonists, progression/LitRPG/system/gamer/isekai/transmigration/CYOA-style power growth, and the sexual kink list; hard exclusions cover slash/m-m/gay, romance, hurt/comfort, fluff, angst, slow burn, RPF, reader, omega, needs-a-hug, bestiality and audience ratings. The local reading list and browser bookmarks are also read at runtime as low-weight read-only sources of additional author/title/folder signals. They are never printed, committed or sent over the network. No preference value is published.

## Success criteria

- Targeted tests for curation, provenance tags, body policy, DB candidates and CLI behavior are green on both engines where relevant.
- A 100-work pilot reports ranked candidates and enqueues 100 body imports.
- The full run queues no more than `min(--limit, preferences.max_bodies)` imports.
- Excluded candidates never reach the body queue.
- The final report names discovered, ranked, excluded, selected, queued, skipped-existing, failed, bytes and shortfall counts.
- Repository gate and estate gates are green at the end of an implementation turn.

## Decisions

- Metadata first, bodies second. This keeps the broad aggregation bounded and avoids another AO3 rate-limit incident.
- Body storage is private: the operator's taste archive is not a reader-facing schema change.
- Shortfall is honest. If fewer than 20,000 works clear the taste rules, the report says so.
- Unsupported sources are skipped with a reason; they are never faked through an untested adapter.
