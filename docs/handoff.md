## Current state — where to resume

The open queue, gates and blocked table are in `docs/plans/roadmap-remaining.md` under `Current state — open work`; that section is authoritative. The standing `/goal` paragraph is `docs/plans/next-goal.md`. Do not re-derive “what is left” from this history file.

**Feature ledger (2026-11-23):** this file is history, not a list of what exists. Current feature counts are in
`docs/FEATURE-LEDGER.md`, generated from `docs/requirements.csv`; row-by-row status and evidence stay in that CSV.
Do not add another “current state” list here.

**Faceted-search brief (2026-11-26):** an outside brief was triaged against that CSV, not accepted at face value.
Seven rows are queued as FS-01/02/03 and FS-04..FS-07 in `docs/plans/roadmap-remaining.md`, ranked by live supply
and use; the rest is already implemented or explicitly parked. See
`docs/spec-amendments/faceted-search-brief-2026-11-26.md`.

## 2026-11-26 — FS-01 crossover facet (9ab9066c)

`crossover:exclude` and `crossover:only` compile to a correlated count of distinct fandom taxonomy nodes; no new column or migration. The SPA offers both as mutually exclusive presets, so one click replaces the other mode instead of producing a contradiction. Proof: parser 1/1, dual-engine SQLite 1/1 and PostgreSQL 1/1, mutant exit 101 at `only must keep exactly the crossover`, Search DOM 29/29, live `?q=crossover:only` 200, browser presets replace modes in the URL.

FS-02 is logged as blocked in the roadmap: its role tables are empty and no public source supplies roles; exact unblock and owner are recorded there.

## 2026-11-26 — FS-05 search within results (1c36bb46)

The SPA Search page now has a **Search within these results** field that appends a plain AND term to the active canonical query; it does not add a second filter model. Every run writes the full query to `?q=`, and the server-rendered `/search?q=...` restores it, so a copied link carries facets and scoped terms together. Proof: `Search.test.ts` 28/28, `svelte-check` 0 errors, release build 0, home 200, live browser probe all six assertions true.

## 2026-11-26 — FS-03 include/exclude chips (ead1bbba)

Search filter chips now flip between `field:"value"` and `-field:"value"` with one click; the chip is struck through while excluded and flips back. The Search filter bar is the single include/exclude surface; server-owned QueryStatePanel chips and entity-page `MetadataChip` selection controls were left alone. A direct load of `/search` serves the server-rendered page, so the live DOM proof navigates there from the SPA shell. Proof: `Search.test.ts` 27/27, `svelte-check` 0 errors, release build 0, home 200, live browser requests include -> exclude -> include with all six assertions true.

## 2026-11-26 — FS-04 saved-search discoverability (af11e192)

The saved-view and alert routes existed but the live tables were empty: 0 saved views, 0 alerts. The Search page now offers **Save this search** on any non-empty query and **Save and get new matches** in the zero-result state. It stores the canonical query as a `catalogue` view and attaches one daily alert in the same action. A library view was rejected because it would reinterpret a global search string against shelf/tag/status columns. Verification: `Search.test.ts` 26/26, `svelte-check` 0 errors, live POST saved-views 201, POST search-alerts 200 created, cleanup 204, proof rows 0.

## 2026-11-26 — UI critical pass (ee830b5c)

The full route sweep found three visual systems stitched together: server documents, SPA shell and injected Polaris
chrome. Fixed in place: Archive palette is genuinely dark and its link/footer accent now passes AA (probe
`count=0` after the `:root[data-palette=archive]` override); body state attributes make the injected bar auth-aware
without duplicate Search/Sign in (estate `48d1073`); landing metadata wraps as a row with a two-work-per-author cap
and `Browse all`; reader imported headings become quiet paragraphs and the external `/reader-position.js` adds a
progress rail and saved-position restore while the no-JS document stays readable; standalone controls reach 44px;
headings and empty states are actionable. The expanded 240-candidate landing pool caused a live 408 and was replaced
with a bounded scan. Verification: frontend 34/34, `svelte-check` 0 errors, landing 5/5, reader unit 1/1, release
build exit 0, 148-probe sweep 0 errors and `overflowX=0` everywhere. Row-by-row evidence is `UC-1`..`UC-10`.

## 2026-11-25 — shell UX repair (452b4087)

Signed-out Library card, 4-link + More nav (a screenshot caught six group menus still inline at 812/522px; collapsed to one More menu), styled header controls, dismissible PWA notice, mobile account door, compact footer, 16px body copy and 44px hit boxes. 24 targeted tests green, svelte-check 0 errors, Vite build green; live 1440/390 measurements clean. Rebuilt the embedded Rust binary because no assets.dir is configured in production.

## 2026-11-25 — final blocker retirement

TA3-2/3/B1/B2/B4 retired per owner decisions and live proof. Discord global registration verified with 1 command / 13 subcommands. Only TA3-B3 remains blocked.

## 2026-11-25 — unblock pass

Closed M64-06 (live data now has 4 tagged-and-engaged works) and M45-75-live (pawchive canonical hit; ao3 200/5). Fixed the NULL source-clock 500 found while probing ao3; rebuilt and deployed. Remaining blockers need owner actions; see `docs/OWNER-UNBLOCK-STEPS.md`.

## 2026-11-25 — M45-75 follow-up closed

Hit carries `source_key`/`source_updated_at`; CLI prints them. Sort proof now mutable and mutant-red. Migration 0220 adds the source/work index after EXPLAIN showed a scan; live plan uses it twice. Build 1m46, migrate 0, health 200, `0.1.0+af2441ca.dirty`.

## 2026-11-25 — M45-75 query vocabulary shipped

Dual-engine suite 1/1 on SQLite and PostgreSQL; domain and CLI targeted tests 1/1; app check exit 0. Release build 2m20 with the estate gate, deployed at `0.1.0+01c32f62.dirty`, health 200. Live anonymous `me:rated` -> 401 and `sort=nope` -> 422. Canonical public query returned 0 because source-linked works are not public published; parked with the exact unblock, no visibility change.

## 2026-11-25 — M45-77 live quorum closed

Owner-authorised generated credentials seeded three independent curator accounts; proposal `57faea22` passed 2/2 with two distinct voters, an active group exists and the public probe printed `PASS` (3 = 3). Credentials are 0600 in `/personal/secrets/estate/darkhold-curator-staff.env`. AO3 re-probe still 429. State table and ledger updated in the same commit.

TA3-4 is shipped: TheForce joined the XenForo family with a recorded public fixture, robots verdict, catalogue claim and a 10-second adapter pace. TA3-2's per-source import guard is in `10b610de` and deployed with `darkhold.service` plus `darkhold-import-worker@2..4.service` (4 active workers). Live probe: no source_key had more than one leased import; succeeded jobs rose 450→452 in 30 s; queued stayed 21,122 as AO3 429 retried. Live meta `0.1.0+d97e7f34`. This pass verified the final Pawchive BFS and hardened AO3 refetch: `--offset` (`2108da6f`) selects a bounded starting row and already-tagged works are skipped (`136c771b`). Live paced batches at offsets 0/20/100 applied with `works_tagged=0` because AO3 returned HTTP 429 on public probes; TA3-3 is parked with that exact unblock. The CLI was rebuilt and installed on thinkcentre; the server binary was untouched, so live meta remains `0.1.0+79f1de8c`.

## 2026-11-20 — TA2 complete

Deploy revealed four real defects, each fixed with a targeted red/green/mutant cycle: an `AuthorFrontier` source was skipped because it has no static listing; the `/recommended` suffix was mistaken for the author id; the catalogue's relative `recommended_template` was passed to `SafeFetcher` as if it were absolute; and full author URLs returned by the Pawchive parser were wrapped into the URL a second time. Pawchive BFS now walks from the single Dark Wolf Shiro seed and hands the adapter one clean author URL.

Live repair before body archiving: `dh catalogue --group-chapters --account … --yes` reported `scanned=27464 groups=464 merged=809 urls=464 applied=true`; Pawchive library rows fell from 27,464 to 26,655 and 464 keeper rows carry `source_urls` in provenance. A named 3.5 GB dump was taken first and verified with `pg_restore -l`.

The daily body wrapper ran successfully and wrote `~/.local/state/darkhold/archive-bodies.latest`. It uses `dh archive --db-url … --account … --pseud …` with the private 0600 preferences file copied to the deploy tree; discovery stays body-policy `skip`, selected works use `store`. The remaining 20k default in `bodies --limit` was removed: the flag is optional, unbounded by default.

Final deploy evidence: live meta `0.1.0+79f1de8c`; `dh` CLI rebuilt at `9608c16b`; daily wrapper ran with `body-select queued=11364 skipped_existing=9341 skipped_unfetchable=24232 cap=18446744073709551615 applied=true` and wrote its freshness artefact. The 7,924 invalid `archive` imports from the earlier run were deleted after the named dump; no `archive` import remains. A forced final `source_sync` job is queued behind a saturated single worker; the live pass before the last normalization fix already discovered 26 authors with no skipped source, and the new normalization is covered by a targeted test.

Acceptance commands and evidence: `cargo test -p darkhold-app --test syndication_frontier` 7/7; `cargo test -p darkhold-cli --test archive_cli` 3/3; `cargo test -p darkhold-db --test pawchive_grouping` SQLite+PostgreSQL green; `cargo test -p darkhold-scrapers --test efiction_fixtures` green. One mutant per fix reddened and was restored. Full estate gate was not rerun after the first 420 s timeout, per the targeted-test instruction.

## 2026-11-20 — TA2 code complete; final deploy in progress

All TA2 code rows are implemented on `master`: no 20k ceiling, Pawchive chapter grouping with a repair verb, private-taste body enqueue test, daily body wrapper/timer, live 24-hour syndication config with the one Dark Wolf Shiro seed and 120-day Pawchive window, and four new eFiction-family adapters with fixture-backed host claims. The other 20 catalogued sources have written coverage dispositions in `docs/SPEC-TA2-DAILY-AGGREGATION-AND-BODIES.md`. Targeted tests and one mutant per code row are green/red/restored. TA2-1 remains open until the release build and live restart finish.

## 2026-11-20 — TA2 roadmap opened; code work pending

New decisions: one Pawchive BFS seed at `https://pawchive.pw/patreon/user/13212571/recommended`, depth 3 only; Pawchive chapter submissions collapse into one library work; Pawchive admits only updates older than 120 days; AO3/QQ/SV/SB and other supported archives check daily without that window; bodies are archived for works with liked tags and no excluded tags; the 20k ceiling is removed; adapter coverage expands fixture-by-fixture. Spec: `docs/SPEC-TA2-DAILY-AGGREGATION-AND-BODIES.md`. Plan: `/home/alvaro/.hermes/plans/2026-11-20_214101-ta2-daily-aggregation-and-bodies.md`. Deploy happens once after TA2 code, not twice.

## 2026-11-20 — TA1 code shipped, live pilot pending

Shipped: pure taste scorer (`crates/taste/src/curation.rs`), provenance-tag preservation, per-job `body_policy` (`skip` for discovery, `store` for selected works), two-engine candidate query (`crates/db/src/taste_curation.rs`), and `dh archive rank|bodies`. Private preferences are a 0600 gitignored JSON expanded from the operator's AO3 searches, with low-weight runtime signals merged from the local reading list and bookmarks; those private files are never printed or committed. Targeted tests are green and one mutant per code task failed and was restored. Live 100-work pilot and full 20k run are still pending.

## 2026-11-20 — TA1 cross-archive taste archive started

Spec: `docs/SPEC-TA1-CROSS-ARCHIVE-TASTE-ARCHIVE.md`. Ledger row `TA1` added. Scope: metadata from all supported catalogued sources, private 0600 taste preferences, per-job body policy, 100-work pilot before a 20k cap, shortfall instead of padding. No reader-facing schema blobs and no source bypass.

## 2026-11-20 — M63-32 like-count half blocked

Pawchive's public API has no likes/endorsements field or endpoint. Its documented public surface has 14 operations; the five favorite operations are account-session `cookieAuth` endpoints. The public post page shows a Favorite button with no count. Views/comments are not a substitute. Clearing condition: an authenticated Pawchive favorites/likes export or an owner-provided session credential stored out of tree; no credential value belongs in chat or the repo.

## 2026-11-20 — M63-32 archive full pass complete

Background backfill finished: archive 28,488 scanned, 11,885 works tagged, 329,455 assertions, 57,724 nodes. Live coverage is archive 11,885 / 28,488 and pawchive 19,028 / 27,464. SQL over provenance proves those are exactly the rows carrying stored claims: archive 11,885 / 28,488 with non-empty `tags`; pawchive 18,890 / 27,464 with fandom/tags/characters (plus 138 rows tagged before this pass). The stored-metadata tag half is complete for archive and pawchive. AO3 still needs source re-fetch; 10 / 2,082 are tagged from the first applied batch.

## 2026-11-20 — M63-32 AO3 live batch

AO3 re-fetch dry run scanned 10 works and parsed 58 tag assertions without writing. Applied batch of 50: 10 works tagged, 303 assertions, 46 nodes created. AO3 coverage moved 0 -> 10 / 2,082 under the same named pre-write dump. Archive backfill was still running.

## 2026-11-20 — M63-32 AO3 re-fetch tool shipped

`dh catalogue --source ao3 --refetch --limit N` now fetches source metadata through Darkhold's own adapter registry and `SafeFetcher` with 1.2 s/2 s pacing, then writes the adapter tags as source assertions through the same idempotent helper. Dry-run by default. The earlier HTTP 429 cleared on a later probe.

## 2026-11-20 — M63-32 AO3 fetch probe

AO3's stored provenance has no tags, so its 2,082 works need a source re-fetch. A representative source URL answered HTTP 429 from this host, so that re-fetch is rate-limited right now and must be retried later with the site's own adapter and backoff. Archive backfill was still running; interim coverage was archive 2,922 / 28,488 and pawchive 19,028 / 27,464.

## 2026-11-20 — M63-32 live backfill started

Fresh named scoped dump taken before any write: `/personal/backups/darkhold-20261120-163011-m63-32-tag-backfill.dump` (1,006,466 bytes, `TABLE_DATA=2` for `canonical_entities` and `work_tags`, verified with `pg_restore -l`). Applied pawchive 500 (332 works, 624 assertions) and archive 500 (216 works, 6036 assertions), then launched full pawchive and archive passes. Pawchive full pass finished: 27,464 scanned, 18,890 works_tagged, 38,725 assertions, 3,102 nodes. Coverage moved pawchive 18,724 -> 19,028 and archive 0 -> 2,497 while archive was still running.

## 2026-11-20 — M63-32 live dry run

`dh catalogue --source archive --limit 50 --dry-run` on the live instance scanned 50 rows and parsed 865 source-backed assertions without writing (`applied=false`). The dry-run report now counts `works_tagged` as well. No live write has been made; the next step is a fresh named dump followed by bounded application.

## 2026-11-20 — M64-14 deployed live

`estate.py deploy darkhold --yes` completed: build exit 0 (1019 s), migrate exit 0, restart exit 0, five probes exit 0, `DEPLOY_EXIT=0`. Live `/api/v1/meta` reports `0.1.0+9c34e649`. No migration or data write was part of this deploy, so no new `pg_dump` was taken; the earlier named dump remains the rollback point.

## 2026-11-20 — M63-32 partial: provenance tag backfill tool shipped

`dh catalogue --db-url … --source KEY --limit N --yes` now backfills source-backed tags from `library_items.provenance_json` without re-fetching source pages. Dry run writes nothing; a real pass writes visible `Claimant::Source` assertions into fandom/character/relationship/tag nodes and is idempotent. Tests 2/2 SQLite and 2/2 PostgreSQL; dry-run mutant `left: 7 / right: 0`, restored from /tmp. Live backfill not run; like counts still blocked by Pawchive's API.

## 2026-11-20 — M64-14 served-feed rerank shipped

The discovery feed now reranks its final candidate order against the signed-in reader's fandom signals when local Ollama is configured (`DARKHOLD_OLLAMA_URL`, `DARKHOLD_OLLAMA_MODEL`). It runs after the pull histogram and returns the caller's order on unset config, no taste profile, anonymous reader, provider failure or DB failure. New `work_summaries_for` reads summaries on both engines; provider calls carry a 2-second timeout.

Evidence: `crates/app/tests/m64_14_feed_embedding.rs` 2/2 on SQLite and 2/2 on PostgreSQL. The fake provider moves the matching work first, and the mock HTTP endpoint proves the served feed calls the configured provider. Removing the `feed_for` call failed with `the served feed never called the configured provider`; restored from `/tmp/m64-14-feed-wiring-clean.rs`. Clippy `-D warnings` exit 0.

## 2026-11-20 — M45-43 closed by live slash-command proof

Discord bot redesigned to Ascend-style signed interactions. `/darkhold help` works in Gaming Omniverse; endpoint saved and verified; MESSAGE_CONTENT off. Icon and deployment evidence are in the darkbot repo.

## 2026-11-20 — M45-74 closed

Owner accepted the sidecar census. Existing `seed read-list` write path proved by its acceptance suite (1/1) with provenance mutant red/restored. No census values recorded in the repo.

## 2026-11-20 — M64-14 Ollama provider shipped

Local Ollama `/v1/embeddings` adapter is wired with env config, model naming and degrade-on-failure. Mock HTTP test 2/2; empty-vector mutant red/restored. Served-feed reranking remains.

## 2026-11-20 — owner unblocks recorded; M63-25 closed

Owner decisions: rename M63-25; Ollama endpoint for M64-14; authorize M63-32 source re-fetch; accept M45-74 census; keep M69-24 held on the foreign shared-crate checkout; remove the >=8 GB deploy gate. M63-25 audit test 1/1 with wording mutant red/restored. Other decisions are recorded in `docs/DECISIONS.md` and the Blocked table.

## 2026-11-20 — all remaining rows blocked; M69-24 condition refined

Suki is clean and already depends on `polaris-taste-source`, so that half of M69-24 cleared. The shared crate itself now has uncommitted `Cargo.lock` and five source files from another session. M69-24 clears when that checkout is committed or stashed; then one versioned passport schema can be added in the shared crate and adopted by both products. Every other remaining row is still blocked; halted per §2.1.

## 2026-11-20 — M29-08 closed

Representative title and body are proven immutable against later joiners; rewrite mutant red/restored. All §29 slices shipped (rate limit, local clustering, frozen text, least-compared arena) on top of M45-01/M45-D00.

## 2026-11-20 — M29-08 least-compared arena shipped

`arena_candidates` now orders by `matches_played ASC, RANDOM()` on both engines. Store 1/1 SQLite + 1/1 PG; random-only mutant red/restored. Only the explicit frozen-representative-text guard remains.

## 2026-11-20 — M29-08 clustering shipped

Suggestions now attach to the closest idea-stage card via shared local Jaccard, with `clustered_by` distinguishing exact-title, local-jaccard and new-card; representative titles are not rewritten. Domain 1/1, store 1/1 SQLite + 1/1 PG, mutant red/restored. Remaining: frozen representative text and least-compared arena.

## 2026-11-20 — M29-08 suggestion rate limit shipped

`[roadmap].max_suggestions_per_day` defaults to 3 and is enforced through a two-engine `count_suggestions_since`. Count 1/1 SQLite + 1/1 PostgreSQL, mutant red/restored, config 76/76, clippy clean. Remaining: clustering, frozen representative text, least-compared arena.

## 2026-11-20 — M44-02 closed, M29-08 remaining scoped

M44-02 was already delivered as M45-01/M45-D00: six roadmap routes, MaxDiff→Elo, 55-test acceptance suite. Added a retired-doc-title alias so the closed row stops generating an idea; catalogue 93 ideas, ideas test 3/3. M29-08 keeps only the §29 additions — suggestion rate limit, clustering, frozen representative text, least-compared arena — and those are the next slices.

## 2026-11-20 — M66-08 local edges shipped

`dh share-queue --queue FILE suggest --for PATH` derives recommendations from co-queued paths on this device only; no network. Targeted share suite 6/6, mutant red/restored, clippy clean. No full gate per owner rule. M69-24 is blocked until the suki checkout is clean (foreign uncommitted migration/service edit).

## 2026-11-20 — M66-08 offline queue shipped

`dh share-queue --queue FILE add|list|drain` queues direct-share paths locally with no network call; `drain` keeps unreachable items. Queue file mode 0600 because the URL is a bearer capability. Tests 5/5 including traversal guard mutant red/restored; First full gate failed on roff-escaped `share\-queue` in the man page and a stale M71-07 idea row; test normalised, catalogue regenerated to 94 ideas, targeted tests green. Retry stopped by owner efficiency correction (`proc_bd3f087f86a4` killed): no more hour-long gates mid-turn. Targeted share 5/5, completions 4/4, ideas 3/3 are the evidence. Row remains planned for local edges and the later seeding threat model.

## 2026-11-20 — M71-07 closed by census

Four admin SPA surfaces, all with `StatusMark` and `AlertBar`; no reader-facing component imported by the console. `/admin` is a server document with live counts, named red states with fix links, and 404 for non-operators. Evidence: admin_overview 1/1, frontend 33/33, route_inventory 6/6, ideas 3/3, full gate `/tmp/gate-all-m71-14-close-bc48e323.log` green. Live deploy remains under the host-memory block.

## 2026-11-20 — M71-14 closed

Inline typography edits now offer "Set as default", and the gravity dial reports its level with a bodyless reset. RED first for both, GREEN ReaderSettings 2/2 and gravity 1/1 SQLite + 1/1 PostgreSQL; dirty-always-false and source-always-instance mutants red/restored. First full gate `24d35749` failed only on the now-built M71-14 idea row; catalogue regenerated to 96 ideas and targeted `ideas_match_requirements` 3/3. Full retry `/tmp/gate-all-m71-14-close-bc48e323.log` `GATE_ALL_EXIT=0` at `bc48e323`. Live deploy remains under the host-memory block.

## 2026-11-20 — M71-14 typography UI and bodyless reset

The provenance rows are now visible in the reader settings and their Reset button calls `DELETE /api/v1/settings/typography`; a version-0 reload applies instance defaults. RED first for both route and component, GREEN 1/1 SQLite + 1/1 PostgreSQL, route_inventory 6/6, ReaderSettings 1/1; both mutants red/restored. Full gate `/tmp/gate-all-m71-14-ui-defa0933.log` `GATE_ALL_EXIT=0` at `defa0933`. The "set as default" inline-change affordance remains in the row; deploy stays under the host-memory block.

## 2026-11-20 — M71-14 typography provenance and reset

`GET /settings/typography` now names each field's level; `PATCH` accepts `{reset:true}` and deletes the account row through the existing reset function rather than writing defaults under an account source. RED first, then 1/1 SQLite + 1/1 PostgreSQL; two valid mutants red/restored from `/tmp/m71-14-typography-clean.rs`. Full gate `/tmp/gate-all-m71-14-typography-e50b639b.log` `GATE_ALL_EXIT=0` at `e50b639b`. UI affordance remains in the row. Deploy stays under the host-memory block.

## 2026-11-20 — M71-14 per-work provenance slice

`GET/PUT /feedback/preferences/works/{id}` now reports each field's source and a reset for an explicit per-work override. RED first, then 1/1 SQLite + 1/1 PostgreSQL; source mutant red/restored from `/tmp/m71-14-clean.rs`. Full gate `/tmp/gate-all-m71-14-provenance-67e523c6.log` `GATE_ALL_EXIT=0` at `67e523c6`. Typography/gravity and the UI affordance remain in the row. Deploy stays under the host-memory block.

## 2026-11-20 — M71-07 economy register added

`AdminEconomyFlows` now feeds the shared `AlertBar`: an undeclared credit source is named with its next step and a Fix link to the unclassified table. RED 1/10 before implementation, GREEN 10/10, condition mutant RED 2/10 and restored from `/tmp/m71-07-clean.svelte`. No new API or store query. Full gate `/tmp/gate-all-m71-07-economy-5a56e16c.log` `GATE_ALL_EXIT=0` at `5a56e16c`. Live deploy shares the M64-15 host-memory block.

## 2026-11-19 — M70-13 orphan ratchet 27 → 26

Deleted `external::rebind_bot_token`: no app caller, and its own history says it used to rewrite a registration's credential on every confirmation. Keeping a stable registration is the multi-link guarantee, so the right pass was deletion, not a new operator door. Ratchet was RED first (1 passed, 1 failed), GREEN after baseline regeneration (2/2, 26 orphans); re-add mutant RED and clean restore GREEN from `/tmp/m70-13-clean.rs`. Full gate `/tmp/gate-all-m70-13-60251e45.log` `GATE_ALL_EXIT=0` at `60251e45`. First gate `fb888642` failed only on two now-built idea rows; regenerated catalogue and targeted idea suite 3/3 before retry. No live surface moved; no deploy.

## 2026-11-19 — M64-15 closed

`POST /api/v1/operator/experiments/{surface}/preview` accepts `{pseud, algorithms?}` for `discover.feed`, runs the chosen reader's taste ranker on the read side, and writes no arm, impression, shadow sample or slot. Both engines 3/3, arm-guard mutant red/restored from `/tmp/m64-15.bak`. Final full gate `/tmp/gate-all-m64-15-retry-a187730c.log` `GATE_ALL_EXIT=0`, all eight stages plus ledger/repo green. The first gate failed only on the missing pre-existing `DARKHOLD_JOBS_TEST_DSN`; targeted CLI retry 117/117. Ledger closed; deploy is BLOCKED on host memory, not code: named dump `/personal/backups/darkhold-20261119-224442-d80c113d.dump` verified `TABLE_DATA=411`; estate build was SIGTERM'd by earlyoom with rustc at 6.9 GB RSS and 1.5 GB available. Clears at ≥8 GB available with no foreign compiler/crawler, or owner authorises a lower-peak profile. Next unblocked row: M71-07..

## 2026-11-19 — M70-14 closed against the existing worker

The requirement was already satisfied by `crates/app/src/worker.rs` and `webhook_delivery.rs`; the ledger now records that rather than opening a second outbound path.

## 2026-11-19 — M64-14 embedding domain slice

Summary-first selection, kernel dense cosine, and degrade-to-disabled ranking shipped at `5a4d72f2`; full gate green. Not wired to a feed because no provider is configured — the row remains planned for the adapter and served surface.

## 2026-11-19 — M64-13 ledger close

The engagement-overlap row was already deployed at `0.1.0+64c30a83` with a green full gate, but its ledger status stayed `planned`. Corrected to `implemented-verified-e2e` with the existing gate and dump evidence.

## 2026-11-19 — M70-13 orphan pass and reserved decisions

28 → 27 orphans; full gate green at `41da552a`. M63-33, M71-20 and M71-03 closed by recorded decisions. M45-43 waits only on the Discord portal intent toggle and a guild invite.

## 2026-11-19 — M63-23 local operator dump and health shipped

`dh admin backup --to -` and `dh admin health` are implemented. Full gate green at `8be69b22`. The first gate failed on stale built-idea rows after M73-03/M73-04 closure; regenerated catalogue and re-ran green.

## 2026-11-19 — owner unblocks

Discord credential stored out-of-tree and verified; darkbot gateway wired and reached READY, with MESSAGE_CONTENT opt-in until the developer portal enables it. Ascend unfrozen and adopted `polaris-affinity` by deletion. `polaris-eval` drift fixed and orbital adopted the shared affinity arithmetic. No darkhold deploy: these are separate products.

## 2026-11-19 — M71-13 coverage provenance split shipped

Dimension coverage now splits works into hosted, referenced and known using the same rules as a work's provenance badge. The numbers come from the existing one-statement aggregate on both engines; the tag/fandom API and the document tag page publish the same words.

Decision: no separate fandom/source document route exists here, so the shared `coverage` object is the source of truth and no second split was hand-written.

Proof: two-engine DB 1/1, API/document 3/3, hosted-predicate mutant red/restored, full gate `/tmp/gate-all-m71-13-close-f6e4aceb.log` `GATE_ALL_EXIT=0`, deploy `0.1.0+f6e4aceb` with named dump `TABLE_DATA=411`, estate `ESTATE_DEPLOY_EXIT=0`, unit active.

## 2026-11-19 — M64-13 engagement-overlap door shipped

`GET /api/v1/works/{work_id}/engagement-overlap` publishes the size of the intersection of two works' reader clusters, after the `K_OTHERS = 10` floor. Below-floor answers are `{fewer_than:10}` and no reader id leaves the database. The row remains partial until the query is wired into the three named ranking surfaces.

Proof: both-engine DB test 1/1, route test 1/1, route inventory 6/6, floor mutant red/restored, full gate `/tmp/gate-all-m64-13-64c30a83.log` `GATE_ALL_EXIT=0`, deploy `0.1.0+64c30a83` with named dump `TABLE_DATA=411`, estate `ESTATE_DEPLOY_EXIT=0`, unit active.

## 2026-11-19 — M64-12 co-read transitions shipped and deployed

`GET /api/v1/works/{work_id}/also-enjoyed` is live. It derives aggregate co-read edges from signed-in reading sessions, excludes sub-30-second legs, and applies a distinct-reader floor before reporting an edge. The only writer of the source table requires a signed-in pseud; a future bot writer would need an explicit exclusion here.

Proof: both-engine DB test 1/1, route test 1/1, route inventory 6/6, 30-second mutant red/restored, full gate `/tmp/gate-all-m64-12-946f18f9.log` `GATE_ALL_EXIT=0`, deploy `0.1.0+946f18f9` with named dump `TABLE_DATA=411`, estate `ESTATE_DEPLOY_EXIT=0`, unit active.

## 2026-11-19 — M66-08 direct-share local text search shipped

`dh share` now answers `?q=needle` on the capability URL with bounded local text search over the shared directory. Real HTTP test 2/2; query-ignoring mutant red/restored. No deploy (CLI only). M71-14, M64-12 and M64-13 blocks with exact clearing conditions were written into the roadmap.

## 2026-11-19 — M71-07 media health alert bar shipped

The media health console names its red states in the operator alert bar: failing providers by name and dead links with the window date. Fix links reach the existing panels; no automatic repair or new action was invented. `AdminMediaHealth 6/6`, frontend gate ok, mutant red/restored. Row stays planned for remaining console surfaces.

## 2026-11-19 — M66-08 direct file share shipped

`dh share --dir DIR` serves the reader's own files directly under a per-run random URL capability. Loopback is the default; `--lan` is an explicit exposure choice. No relay, DHT, discovery, global namespace or site account. Real HTTP test 1/1; wrong-token mutant red/restored. Row stays planned for the remaining local-engine and seeding milestones.

## 2026-11-19 — M63-25 `dh audit` CLI clause shipped

`dh audit --since 7d` now works: relative windows are resolved client-side to RFC 3339 because the server deliberately refuses `7d`. The text output says "what your account did" and names the missing token attribution rather than pretending the schema has it. Fake-origin test 1/1, unit 1/1, mutant red/restored. Row stays planned for the DDL/backfill decision that would add a token id to the write path.

## 2026-11-19 — M71-13 fandom facet coverage line shipped

The fandom facet leads with `N of M works are tagged (P%)` from the API's existing aggregate, not a typed-in headline. No API or store change. `FacetPanel 4/4`, `gate.sh frontend ok`; percentage mutant red/restored. Row stays planned for the hosted/referenced split, which needs an origin concept first.

## 2026-11-19 — M64-11 lists-containing door shipped

`GET /api/v1/works/{work_id}/lists` is live. It answers the `story.lists_containing` registry surface from the existing `user_lists`/`user_list_members` adjacency, publishing only public lists. No new table, migration, config key or session is needed, and private lists are not named or counted.

Proof: two-engine DB test and route test green, visibility mutant red/restored, route inventory 6/6, full gate `/tmp/gate-all-m64-11-fdc22bd1.log` `GATE_ALL_EXIT=0`, live HTTP 200 probe, deploy `0.1.0+fdc22bd1` with named dump `TABLE_DATA=411`, estate `ESTATE_DEPLOY_EXIT=0`, unit active.

## 2026-11-18 — M64-09 taste gravity on the remaining rails shipped

`GET /works/{id}/similar` and `GET /api/v1/library/because-you-shelved` now apply the same bounded taste-gravity mixture as the feed. A shared helper resolves the surface's knobs, the reader's dial and the reader's own shelf signal, then reorders the engine's output without filtering it or changing any score. Anonymous callers and failed signal reads keep the original order. No new route, schema, config key or deploy-time migration.

Proof: pure gravity tests 2/2, reader_surface/library suites green, zero-strength mutant red and restored, full gate `/tmp/gate-all-m64-09-47e855dd.log` `GATE_ALL_EXIT=0`, deploy `0.1.0+47e855dd` with named dump `TABLE_DATA=411`, estate `ESTATE_DEPLOY_EXIT=0`, origin/edge build verified, unit active.

## 2026-11-18 — M64-08 fandom graph surfaces shipped and deployed

Two new anonymous API doors reach the operator's declared map: `GET /api/v1/tags/{tag}/cross-fandom?from=<fandom>` and `GET /api/v1/authors/{author}/similar`. Both publish the map version and empty lists rather than 404 for unmapped inputs. The trope door requires `from` because a trope alone does not name a source fandom; the author door derives source fandoms from published/public works and excludes the source author. Both queries exist for SQLite and PostgreSQL, carry the operator's edge weight through, and never expose private or draft works.

Proof: route tests 3/3, DB 4/4 on both engines, route inventory 6/6, source-author-exclusion mutant red and restored, full gate `/tmp/gate-all-m64-08-d9e3519f.log` `GATE_ALL_EXIT=0`, deploy `0.1.0+d9e3519f` with named dump `TABLE_DATA=408`, estate `ESTATE_DEPLOY_EXIT=0`, origin/edge build verified, unit active.

M64-06 was demoted: `work_tags` joins zero engaged rows, so the map cannot be proven through a WorkTags surface until M63-32's backfill lands. The `m65_module_host` idea floor moved from 100 to 95 after three legitimate retirements; adding a requirement only to keep a parser floor green would have been worse.

## 2026-11-18 — SSHFS mirror cleared

`/home/alvaro/code/rust/darkhold` fast-forwarded to `66aad53d` and is clean. The earlier `folio_wait_bit_common` hang did not recur after `origin/master` was fetched with `GIT_CONFIG_GLOBAL=/dev/null`; only the `merge --ff-only` needed a longer timeout (90s). No remaining mirror block.

## 2026-11-18 — M63-30 Calibre interop shipped

`dh lib --calibre <path>` reads a Calibre `metadata.db` read-only and emits the seed grammar `darkhold seed read-list` already parses. Rated books only, stable author order, 0–10 ratings left unchanged so the server remains the only owner of star conversion. The default prints a preview; `--out <path>` writes the same text locally. No crate/RPC link, no HTTP route, no schema change, no deploy.

Proof: full gate `/tmp/gate-all-m63-30-063badea.log` `GATE_ALL_EXIT=0`, all eight stages ok at `063badea`; integration 1/1 with a source-DB byte-hash check; unit 1/1; separator mutant red and restored from `/tmp/dh-calibre.bak`; clippy clean.

## 2026-11-18 — M63-26 `dh stats` shipped

`dh stats --by week|year [--year YYYY] [--share]` reads two private surfaces the site already has: `GET /api/v1/me/streak` for current/longest streak and freezes, and `GET /api/v1/me/passport` for the origin-signed contribution snapshot. The passport payload is base64url-decoded locally; the CLI does not restate a signature check that belongs to the receiving instance. `--share` drops handle, origin and exact issue time and keeps totals only. No new route, schema, migration or deploy.

The power-user sweep's "operation log (imports, tags applied, reports filed)" wording has no schema behind it in either engine; works/revisions/comments are the counts that exist. Recorded in the roadmap so a future operation-log surface can replace the adapter instead of being duplicated.

Proof: full gate `/tmp/gate-all-m63-26-ba364ea5.log` `GATE_ALL_EXIT=0`, all eight stages ok at `ba364ea5`; integration 3/3, unit 3/3, identity-leak mutant red and restored from `/tmp/dh-stats.bak`, clippy clean.

## 2026-11-18 — M63-29 `dh tui` shipped

`dh tui` is now a command in the same binary, compiled only with `--features tui`. Without the feature it exits 2 with a sentence naming the missing feature; with it, an alternate-screen prompt dispatches each line back to `dh --json`, so the query grammar, saved views and exit codes are not restated. `tui` inside the prompt is refused by name; `q` restores the terminal. Default-feature refusal 1/1; `tui`-feature integration 2/2 and module 2/2; the feature-gate mutant (exit 0 instead of 2) reddened the refusal test and was restored from `/tmp/dh-tui.bak`. Full gate: `/tmp/gate-all-m63-29-18edf198-jobsenv.log` ended `GATE_ALL_EXIT=0`, all eight stages ok, with `DARKHOLD_JOBS_TEST_DSN` set for the concurrently merged row 409 A3. No deploy: feature-gated CLI only. M73-03 and M73-04 remain Blocked with their clearing conditions.

## 2026-11-18 — concurrent jobs row integrated into the CLI layering gate

A concurrent session pushed `a8a6b8b5` (row 409 A3) during the M63-29 full gate, adding `darkhold-db` and a path dependency on `polaris-jobs` to the CLI. That made `layering` red and the path unavailable in this checkout. The shared crate exists on the thinkcentre mount, so `/home/alvaro/code-local/estate/crates` was symlinked to `/home/alvaro/mnt/thinkcentre/personal/documents/code/estate/crates` (environment fix, not a repo change). `layering.rs` now permits `darkhold-db` only when every import is confined to `src/jobs.rs`; the new guard reads the manifest and the source files. Targeted suite: 3/3. No other session work was reverted.

## 2026-11-25 — H1/H3 closed; M45-76 and M45-77 deployed

Release build from `d2b972b7` with `LTO=off`/`codegen-units=16` finished in 3m16s with both
app and CLI exits 0. Frontend rebuilt first (`index-DeJTFeVo.js`, `✓ built in 19.93s`) and the
served bundle matches `frontend/dist`. A works-only dump was verified before DDL; migrations
0218 and 0219 applied (216 total). Live meta is `0.1.0+d2b972b7` with
`works.auto_abandon_after_months=4`; 15,220 abandoned works carry a seeded
`completion_marked_at`; the trope proposal and completion tables exist. Public
`GET /api/v1/tropes/proposals` returns 200 with an empty list, while the sweep, vote and
completion doors answer 401 anonymously. The staged public probe was fixed after its first
live run exposed a shell-quoting `NameError`, then printed the five seed groups. Lifecycle
units are installed under the user manager with the timer disabled, per the plan's dry-run
rule. The three build swapfiles were deleted. The only M45-77 half still open is a real
third curator: the two eligible accounts plus fixture TL1-2 accounts cannot form an
independent quorum. TA3-2 remains queued (15,055 jobs / 14,338 imports) and AO3 still answers 429.


## Canonical scraper adoption (2026-11-26)

Commit 6c9e2469: AO3 work metadata parsing now comes from the family
`fanfic-scrapers` library (2db9be6). Darkhold retains wall detection, chapter
index keys, UTC date policy, source manifest and body sanitisation. Targeted
verification: `cargo test -p darkhold-scrapers --test ao3_fixtures` → 11 passed,
0 failed. No deploy accompanied this refactor; the live binary is unchanged.
