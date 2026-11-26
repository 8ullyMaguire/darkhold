# Suki → Darkhold crosswalk triage — 2026-11-26

**Status:** triaged, all candidates parked · **Source:** owner request against `10-Projects/reviews/`
and Suki's `docs/FEATURES.csv` (320 features, not the 233 in the 2026-11-17 review).
**Authority:** `docs/requirements.csv` (1,055 rows measured this pass — the 1,039 in the
faceted-search amendment is stale) and `docs/spec.md`. This note records what was checked and what
was refused; it promotes no row into the ledger and changes no code.

## Rule for cross-project borrowing

A feature list from another repo is a claim set, not a gap list. Every candidate was first checked
against the Darkhold spec's 643 headings, then body-grep'd through the full spec, all 1,055
requirement rows and the 200-line idea bank. Only candidates with no match in those three sources
survived. Supply was measured against the live database; a surface with no rows is ranked below one
that has them, and an empty surface is a reason to fill it before building more on top of it.

## The owner's feedback / auto-promote idea: three answers

1. **Already in Suki.** `docs/SPEC.md` §594 and its 2026-11-26 addendum specify per-surface ranking
   arms, `ranking_arm` / `ranking_match` / `ranking_feedback`, explicit "good order / bad order",
   "show the other version", a side-by-side two-pane arena, per-user vote caps, and promotion only
   after a per-surface interval clears. It is in the spec, not yet in the review's feature table.
2. **Already built here.** `M64-01` canonical surfaces with named algorithms, `M64-02` sticky
   assignment and append-only impressions, `M64-03` daily per-arm snapshots, `M64-04` automatic
   promotion on a sequential-test win with guardrails, minimum sample and seven days plus automatic
   demotion, `M64-15` operator hold/manual promote/preview-as-persona, `M64-16` offline
   pre-registration. Do not re-spec this.
3. **Missing is explicit reader feedback**, and the loop is currently off: no `[discovery]` section
   in `darkhold.toml`, so `experiment_buckets` stays at its default 0 (`M64-02`). Live read-only
   census: 0 snapshots, 0 decisions, 0 assignments, 0 overrides, 200 recommendation slots, 24
   accounts. The first action is to enable one surface's buckets and collect impressions.

§28.10's refusal of automatic promotion applies to **roadmap cards**, not ranking algorithms;
`M64` is owner-sanctioned. The binding rule here is §47.4: earned and incentivized interactions are
separate rows, and explicit ranking feedback must not become an earned taste signal by inference.

## Already covered — do not re-propose

| Suki cluster | Darkhold owner |
| --- | --- |
| Arena, pairwise taste, one vote per account | §29.2 MaxDiff, `M45-02`; ranking arms must not reuse the content arena |
| Ranking evaluation, experiments, promotion | §47, `M64-01`–`M64-16`, `M64-16` offline ladder |
| "Why this picture", "Why first", explain | §16.1 `reason`, §33.3 `SlotExplanation`, `M63-22` |
| Saved searches, watches, alerts, zero-result rescue | §14.2, FS-01…FS-07, `M47-08b` |
| Feature gates, extensions, module registry | §59, `M65-01`…`M65-06` |
| Settings registry, export/import, provenance | §46.6, `M47-08`, `M71-14` |
| Doctor, metrics, backups, restore drill, deploy sweep | §24, §38.7, §25 |
| Provenance and takedown surviving re-import | §11, `M69-04` |
| Per-source baselines and relative popularity | `M63-36`, `M63-37` |
| Aliases, implications, wrangling, tag history | §15.11, `M29-03` |
| MMR, diversity, taste gravity, half-life | §47.6, §16.4, §41.1 |
| RSS/Atom/sitemaps, API keys, browser extension | §23.5, §23.1, §37 / §48 |
| Instance fingerprint and peer similarity | `M69-01`, `M69-02` (already sourced from Suki §16.2) |
| Keyboard-first reading, age gate, erasure | §25.3, §7.3, `M45-51` |

## Parked candidates — ranked

All eight are parked, with acceptance criteria in `docs/plans/suki-crosswalk-intake-2026-11-26.md`.
Ranking rule: extends an existing mechanism, then serves the north star or decision correctness,
then has supply. Gated items sit below ungated ones.

| # | candidate | extends | gate / live supply |
| --- | --- | --- | --- |
| XW-1 | Explicit ranking feedback and side-by-side arm comparison | `M64-01`…`M64-16`, `M64-15` | gated: experiment buckets off, 0 snapshots |
| XW-2 | Reader-facing identification queue with candidates and quorum | §15.1, §15.17 | 8,469 `work_characters` rows |
| XW-3 | Named, switchable negative exclusion sets | §7.5 mutes, §15.7 filters | `work_tag_votes = 0`; start from reader mutes |
| XW-4 | Reviewer calibration against audited ground truth | §19, §34 | gated: `review = 0`, `review_classifications = 0` |
| XW-5 | "Garden" order over least-described works | §15.6, `M69-20` | default off, never in `for-you` |
| XW-6 | Rebuild lock and drift check on decision inputs | §10 worker, `M64-03` | insurance; single worker today |
| XW-7 | Saved Filter revision history and simple insights | §14.2 | gated: `saved_views = 0`, subscriptions = 0 |
| XW-8 | Source-endorsement priors for taste cold start | `M63-37`, `M64-16` | 1,247 of 141,317 rows endorsed; provenance required |

## Deliberately not transferred

- Suki's ±1 votes, favourites, points, quests, leaderboard and recognition ladder: §28.10 refuses XP,
  volume leaderboards, reputation scores and threshold-driven promotion for roadmap cards, and §29.2
  ranks by pairwise comparison.
- Suki's safe/questionable/explicit rating filter, blur and NSFW import posture: a different content
  contract; Darkhold's rating and warning vocabulary lives in §15.7 and §11.14.
- Two rendering stacks and the Askama → SvelteKit migration: a Suki defect, not a capability.
  Darkhold already has one embedded Svelte frontend.
- Image-only mechanics (dHash clusters, subjects/regions, hover preview, progressive full image,
  ZIP downloads): transferred where meaningful into §30/§32 media, not into the written-work reader.

## Measurement record

| measured value | command |
| --- | --- |
| 320 Suki features | `wc -l < suki/docs/FEATURES.csv` minus header |
| 1,055 Darkhold requirements | Python `csv.DictReader` over `docs/requirements.csv` |
| 0 / 0 / 0 / 0 snapshots, decisions, assignments, overrides | `ssh thinkcentre 'sudo -u postgres psql -d darkhold -Atc "select …"'` |
| 200 slots / 24 accounts / 8,469 `work_characters` | same query |
| 0 `saved_views`, 0 subscriptions, 0 `review`, 0 `review_classifications`, 0 `work_tag_votes` | same query |

The live app was mid-restart during the census (`darkhold.service` reported `deactivating
(stop-sigterm)`, nothing on 8081), so no live UI claim is made; the database counts are the evidence
for the gates. Full crosswalk with source references: `~/secondbrain/10-Projects/suki-darkhold-features-worth-adding.md`.
