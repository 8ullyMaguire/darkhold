---
title: "Darkhold reading-list and catalogue quality — scored idea bank"
date: 2026-11-17
project: darkhold
type: ideas
status: intake only — not a queue, no row is committed
---

# Reading-list and catalogue quality — 36 scored ideas (2026-11-17)

Companion to `docs/ideas/2026-11-17-syndicated-trope-frontier-ideas.md` (50 ideas) and
`docs/design/what-i-would-not-build-from-scratch.md` (144 exclusions). Every idea below is new: none repeats
those two banks. Nothing here is implemented.

## State check before the list

Measured this pass, not quoted from a status doc:

- The fanfiction-quality plan (`.hermes/plans/2026-11-16_203519-fanfiction-quality-import-and-seed.md`) was
  **folded into docs only**. `docs/handoff.md:134` and the vault note say so in plain words. No sidecar
  producer, no `seed_read_list.rs`, no `--dry-run` loader, no refresh unit exists.
- Live census is unchanged: `rating 0`, `work_taste_vectors 0`, `taste_profiles 2`, `library_items 141,317`,
  `works 58,042`, `import_intake_queue 0`, `external_seeds 0`
  (psql counts against database `darkhold` on thinkcentre).
- Crawler state: 83,600 stories seen, 66,874 recheck rows, only `fic-catalogue-ingest.timer` installed; the
  loader still has neither `dry_run` nor the forward-only `CASE WHEN EXCLUDED.source_updated_at` clause.
- Separate work DID ship: M74-01 syndicated trope frontier (depth-3 Pawchive BFS, per-source windows,
  public metadata with TL2-only bodies), deployed at `0.1.0+93a22eef`; a live `source_sync` job is
  `succeeded` at `2026-11-17T01:51:42Z`. That covers periodic pulls for declared syndication sources; it does
  **not** cover the reading-list seed or the crawler recheck timer.
- Ascend alert work is done: both health-check copies md5 `646618d35c59488943fb24577098da39`, site 200, host
  disk 70% with 136G free. Only the owner decision on estate row 309 remains.
- Estate rows still open: 309 (Ascend/suki version vote), 391 (bind members to identity/taste/index/comments),
  394 (suki taste channel), 407 (reading-list seed), 430 (rotate four leaked credentials — owner).
- Local darkhold tree has two modified files from another session
  (`crates/app/tests/m71_16_machine_surfaces.rs`, `docs/plans/roadmap-remaining.md`); they are not mine and
  must not be swept into another commit.

So: "everything mentioned this session" is **not finished**. The syndication half is; the seed half is not.

## Scoring

`V` = value against the stated priority (quality content the owner would enjoy), 1–5.
`E` = effort to implement and verify, 1–5 (1 = hours, 5 = days or cross-host).
`score = V / E`, printed to two decimals. **Shortlist threshold: at least 1.20**; below that goes in the
"wouldn't" section. Value is judged against the measured constraint above (zero taste signal, stale rechecks),
not against feature count. The V/E numbers are judgement; the numbers they are derived from are measured.

## The ones I would implement (17)

### 1. Emit a versioned sidecar from the existing read-list parser — score 2.50 (V5/E2) [new, was I1]

Extends: `scripts/parse_read_list.py` prints 218 matched of 268 lines but writes no artifact.  
Why: The 218 rated rows are the only real taste signal on the host; a fixed JSONL sidecar makes the seeder's input reviewable instead of re-parsing a moving file.

### 2. Add `darkhold seed read-list` with dry-run as the default — score 2.50 (V5/E2) [new, was I2]

Extends: `crates/app/src/cli.rs` already has `Command::Seed` and `crates/app/src/routes/reading.rs` already calls `reading::upsert_rating`.  
Why: Live `rating` is 0 while 218 rows are ready; a dry-run-first verb keeps the write path honest.

### 3. Apply the seed through `upsert_rating` and write an audit sidecar — score 2.50 (V5/E2) [new, was I3]

Extends: `darkhold_db::reading::upsert_rating` is the route's own service; no `rating.source` column exists.  
Why: An append-only sidecar gives revocation without a two-engine migration for a one-person prior.

### 4. Prove idempotence by running `--apply` twice — score 2.00 (V4/E2) [new, was I4]

Extends: the seed verb and `rating_for` lookup.  
Why: Second run must write 0 and leave `count(*)` unchanged; a one-run test cannot detect a duplicate writer.

### 5. Make crawler `source_updated_at` forward-only — score 2.00 (V4/E2) [new, was I5]

Extends: `ficcrawler/loaders/lorehaven.py::SQL_UPSERT` currently coalesces the old date first.  
Why: 66,874 recheck rows feed this loader; an older recheck can currently win and make fresh metadata look stale.

### 6. Schedule a bounded daily recheck timer — score 2.00 (V4/E2) [new, was I6]

Extends: `ficcrawler.cli refresh --limit 200 --minutes 20` already exists; only the 10-minute ingest timer is installed.  
Why: 66,874 queued stories and no refresh timer means 'automatically update' is only half true.

### 7. Ranking-intersection acceptance gate — score 1.67 (V5/E3) [new, was I8]

Extends: `dh rec --engine personal --limit 10` and the audit sidecar's `work_id` list.  
Why: 600 recommendation slots with 0 picks; at least 1 seeded work in the top 10 is the only evidence the seed reached a ranking.

### 8. Copy-of-live apply before live apply — score 1.67 (V5/E3) [new, was I9]

Extends: `pg_dump -Fc darkhold` and `DARKHOLD_DATABASE_URL`.  
Why: 218 private ratings must not be learnt on the live DB; the 180 exact / 20 ambiguous / 180 applied rule is already fixed.

### 9. Report unmatched and wrapped rows explicitly — score 1.50 (V3/E2) [new, was I12]

Extends: `scripts/parse_read_list.py` and the sidecar's `status` field.  
Why: 47 unmatched plus 3 wrapped-title rows with no author; guessing them would invent taste.

### 10. Stamp the source file's mtime into the sidecar header — score 1.50 (V3/E2) [new, was I13]

Extends: the read-list sidecar header and `/personal/literature/fanfiction/read.txt`.  
Why: The file was last modified 2026-11-02; provenance must say which version was seeded.

### 11. Keep the refresh queue's coverage gap visible — score 1.50 (V3/E2) [new, was I17]

Extends: `state.sqlite3` refresh count (66,874) against 141,317 `library_items`.  
Why: 74,443 items are not in the recheck queue; a dated number prevents claims that all stories are current.

### 12. Add `ficcrawler load --dry-run` — score 1.33 (V4/E3) [new, was I7]

Extends: `ficcrawler/cli.py::cmd_load` and `loaders/lorehaven.py::load`.  
Why: 141,317 `library_items` make a blind bulk load risky; count arrivals and compare `count(*)` before writing.

### 13. Nightly taste census snapshot — score 1.33 (V4/E3) [new, was I10]

Extends: estate `scripts/taste_census.py`, which currently reports the darkhold line as 0.  
Why: 2 taste profiles and 0 vectors; a dated row per night survives seeding and makes the prior's effect visible.

### 14. Record the match strategy on every audited row — score 1.33 (V4/E3) [new, was I14]

Extends: the audit sidecar written by `--apply`.  
Why: Exact normalised title is the only safe rule; a strategy column makes ambiguity auditable later.

### 15. Resolve seeded works through `dh q --seed` — score 1.33 (V4/E3) [new, was I15]

Extends: `crates/cli/src/rec.rs` seed handling and M45-66 seed resolution.  
Why: 218 seeds are already a seed set; reusing the existing resolver avoids a second similarity implementation.

### 16. Show source freshness on `/api/v1/catalogue/syndicated` — score 1.33 (V4/E3) [new, was I16]

Extends: the shipped public route and the live `source_sync` job.  
Why: The live job succeeded at 2026-11-17T01:51:42Z; readers should see when metadata was last checked, not a silent list.

### 17. Rebuild `work_taste_vectors` with the product's nightly job — score 1.25 (V5/E4) [new, was I11]

Extends: the existing rebuild path named in `docs/ROADMAP-seed-from-reading-list.md` Phase 4.  
Why: 0 vectors today; writing ratings without a rebuild leaves every ranking surface unchanged.

## The ones I would not implement (19)

### 18. Daily 'new since your rated list' digest — score 1.00 (V4/E4) [new, was I20]

Extends: the audit sidecar plus `recommendation_slots`.  
Why: 600 slots with 0 picks; filling a digest before proving the ranking gate puts the cart first.

### 19. Publish the seed count in the operator status tile — score 1.00 (V3/E3) [new, was I18]

Extends: `StatusMark.svelte` and `AdminMirror`.  
Why: 0 to about 218 is a real state change; the console currently has no row for it.

### 20. Persist the `source_sync` run report as JSONL — score 1.00 (V3/E3) [new, was I19]

Extends: the `source_sync` job's existing report payload.  
Why: One live job is observed; a durable per-run file makes the next failure comparable to the last success.

### 21. Monthly quality report of seeded versus ranked works — score 0.75 (V3/E4) [new, was I36]

Extends: the audit sidecar and `dh rec --explain` outputs.  
Why: The monthly digest idea in the frontier bank already covers reporting; a second report is ceremony.

### 22. Add a `rating.source` column — score 0.50 (V2/E4) [new, was I23]

Extends: `rating` has no source and no provenance table.  
Why: One prior needs an audit sidecar, not a migration on both engines.

### 23. Put seeded ratings behind trust levels — score 0.50 (V2/E4) [new, was I34]

Extends: `governance::trust_for` and `body_audience`.  
Why: These are private rows on one account; trust gating would hide the only existing signal.

### 24. Migrate `source_updated_at` to timestamps — score 0.50 (V2/E4) [new, was I35]

Extends: `library_items.source_updated_at` in both dialect migrations.  
Why: Dates are enough to decide newer or older; a migration costs two files and a gate.

### 25. Make the seeded ratings public — score 0.50 (V1/E2) [new, was I24]

Extends: `rating.is_public` defaults false.  
Why: One person's reading history is a prior; publishing it changes consent, not quality.

### 26. Auto-accept ambiguous title matches — score 0.50 (V1/E2) [new, was I25]

Extends: the exact-title matcher over 58,042 `works`.  
Why: `works` has no author column; a guess about taste is worse than an unmatched row.

### 27. Add a FanFiction.net adapter — score 0.40 (V2/E5) [new, was I21]

Extends: `sources` has an `ffnet` row with only 4 items.  
Why: 4 rows do not justify a new adapter while 66,874 rechecks are unprocessed.

### 28. Import the 107-site Ficnexus adapter map — score 0.40 (V2/E5) [new, was I22]

Extends: `sources` already carries 14 adapters and the `[PORT]` tasks.  
Why: The crawler sees 83,600 stories over seven sites; breadth before currentness repeats the old catalogue trap.

### 29. Recrawl all 141,317 items nightly — score 0.40 (V2/E5) [new, was I26]

Extends: `ficcrawler run` and its per-host rate limits.  
Why: The queue already holds 66,874 due rechecks; a larger sweep spends politeness budget, not insight.

### 30. Add embedding similarity search — score 0.40 (V2/E5) [new, was I27]

Extends: `search_works_ast_scoped` and `polaris-search`.  
Why: Tag search has not been measured failing; a vector store is a second engine before the first is ranked.

### 31. AI-summarise every imported work — score 0.40 (V2/E5) [new, was I32]

Extends: 58,042 `works` and 127,474 `units`.  
Why: A summary of a work nobody has rated is inventory, not a recommendation.

### 32. Build a browser capture extension — score 0.40 (V2/E5) [new, was I33]

Extends: the import pipeline and `source_credentials`.  
Why: No evidence the current importer is the bottleneck; an extension adds a second intake path.

### 33. Write a new ranking crate — score 0.20 (V1/E5) [new, was I28]

Extends: estate `polaris-taste-source` and `rec_strategy.rs`.  
Why: If the intersection gate is 0, the bug is wiring; a new crate hides it.

### 34. Write new rows to the frozen Ascend shelf — score 0.20 (V1/E5) [new, was I29]

Extends: the crawler's `load --target lorehaven` and the one-catalogue ruling.  
Why: Two catalogues were deliberately retired; reviving one doubles the update problem.

### 35. Revive Ficnexus as a platform — score 0.20 (V1/E5) [new, was I30]

Extends: the retired checkout and deleted DNS/ingress.  
Why: 216 markdown files and no estate entry; its adapters are already ported.

### 36. Add leaderboards and badges for readers — score 0.20 (V1/E5) [new, was I31]

Extends: `badges` and `live_badges` tables.  
Why: 0 seeded ratings means no judgements to rank people by.

## Where these belong next

Ideas 1–17 are the unimplemented fanfiction-quality plan and belong in
`docs/plans/roadmap-remaining.md` under "Fanfiction quality wires" and estate row 407, in that order.
Ideas 18–20 (operator status, run report, digest) are revisit-after-gate items: they stay in this bank until
the ranking intersection is green. Everything in the second section stays here too — a rejection with a
reason is a decision, and re-proposing it next month is a defect.

Owner-gated, above this list: estate rows 309, 391, 394 and 430.
