# The remaining roadmap, ordered for execution

`docs/requirements.csv` holds 101 planned rows as of 2026-11-06. This file is the order they will be taken in and the
reason for that order, so a later session does not have to re-derive it. The ledger stays the source of truth: a row
moves when `scripts/ledger.py upsert` says so, not when this file says so.

Two rules that decide the order:

* **Things a reader can see come before things an operator can see, and both come before infrastructure.** A row that
  changes what a stranger can read or find is worth more than a row that changes what an operator can tune.
* **A row that gates other rows is taken as soon as its blocker clears.** Nine of the M64 rows read a snapshot built by
  M64-01..M64-03; taking them first would mean writing the same aggregation three times.

## Batch 1 — DONE

| row | what shipped |
|---|---|
| M71-10 | the reader's no-JavaScript guarantee as one test (`crates/app/tests/reader_page.rs::the_reader_needs_no_javascript_at_all`), plus the mutation that proves it bites. The position form it asserts already existed; the attempt to add one was a duplicate and was removed. |

## Batch 4 — ranking (M64 / M69)

Shipped: M69-08 (blend decay, floor, score-first ordering, 0109d288), M69-19 (NPMI, da36de8e), M69-12 + M71-06 (reason +
match percentage, e93f94e6), M64-01 (surface registry, load-time validation, refusals by name, be0aead0).

M64-01 is now **fully tested** (0debecd9): `discover.feed` reads its configured engine set, an engine outside the set
does not run and costs no query, and the surfaces endpoint separates "no engine implements this" from "its table is
empty". Three callers still pass `None` with the reason written at the call site: the concierge queue (`home.curated`),
the draw (`draw.hand`, not yet a registry surface) and recipe runs (recipes compose, they do not read a surface set).

**M64-04 decision half shipped** (91a27078): `domain::experiment_promotion::decide` — mixture martingale, guardrails,
sample floor, seven-day floor, demotion on two consecutive breaches of the same guardrail. The record table shipped in a231f678
(`experiment_decisions` + `evaluate_surface`, promote/demote only, swap written on the row). The swap shipped in 111b7d00/5c9053df: promote installs `surface_algorithm_overrides`, demote clears it, and
discover.feed reads the override before the configured set. The sweep shipped in 5434fc26
(`POST /operator/experiments/sweep?day=`, keyed on (surface, day)). **The one real gap: no guardrail source** — the
sweep passes an empty guardrail history, so auto-demotion is exercised only by tests. Guardrail metrics and the console
(M64-15) remain.

**M64-03 shipped** (3d40065f): `experiment_snapshots` (surface, arm, day) with impressions / readers / guest_readers /
events, `snapshot_day` idempotent so a re-run repairs a day, `GET /api/v1/recommendation/experiments[?day=]`, and `null`
vs `[]` distinguishing "never computed" from "computed with no traffic". The nightly scheduler and M64-04's sequential
test are the next pieces; the latter computes from these counts rather than from the log, which is the row's whole point.

**M64-02 pseud half shipped** (cc6cab63): migration 0166 `experiment_assignments`, `bucket_for` insert-if-absent with
FNV-1a, `algorithms_for_bucket` splitting by position, `[discovery] experiment_buckets`. Still missing from that row and
written in its ledger: **anonymous session arms and the capped share** — a session is not a pseud, so it needs a
session-keyed assignment rather than a row in the same table. That is the next M64 piece, and it is also what M64-03's
per-arm metrics need before an experiment can be run on logged-out readers.

## Batch 2 — the visible contract

Shipped from this batch: M69-08 (blend decay/floor/ordering), M69-12 + M71-06 (reason and match percentage on
recommendation rows), M71-16 (sitemap), M71-04 (provenance), M71-05 (absent-not-empty), M71-10 (no-JS reader),
M71-02 and M45-58 (closed by triage).


Triage of all 96 rows, and the order it implies: `docs/plans/planned-triage.md`.

| row | approach | blocker |
|---|---|---|
| ~~M71-04~~ | **DONE 7b3e8f5f.** `routes/provenance.rs`: four words (`hosted`, `hosted_locked`, `referenced`, `known`), one grouped query per list. Wired into tag browse, search, library bookmarks, work page and tag page. Media and the author's own works list are excluded with the reason in the ledger row. |
| ~~M71-05~~ | **DONE 97d28354.** `test_support::{assert_no_empty_section, assert_names_degradation}`, applied to the three document surfaces; SPA surfaces take it as they migrate (M71-20). |
| M71-03 | dense list rows on browse, search, library, rails. Depends on M71-04's field existing (the row carries provenance, rating, length, status). | M71-04 |

## Batch 3 — audits that can only shrink the tree

| row | approach | blocker |
|---|---|---|
| M70-13 | a checked-in ratchet test: every `pub async fn` in `crates/db` must be called from a non-test source in another crate, or appear in `docs/plans/orphan-queries.txt` with a reason. Measured 2026-11-06: 291 db functions referenced only by tests or nothing. The ratchet fails on new ones and the file only shrinks. `content::list_public_works_by_freshness` — the row's own example — is now called by the landing page. | none |
| M63-33 | reconcile the live database with the migrations: 11 tables the migrations create are absent, 12 legacy tables remain. Needs the live DSN; the deploy is the operator's call. | deploy decision |
| M65-09 | decided **evaluated-and-rejected**: a process-wide "any overrides exist" flag is wrong for a second process sharing the database, and the read it removes is one index probe on a surface that is not hot. The row's own note says "not before a profile shows it". | — |

## Batch 4 — ranking engines (M69)

| row | approach |
|---|---|
| ~~M69-19~~ | **DONE da36de8e.** `taxonomy::related_tags_by_npmi` (log in Rust, one grouped count query, pairs seen once excluded), `GET /api/v1/tags/{tag}/related`, and a Related tags section on the tag page. No nightly matrix needed at this size. |
| ~~M69-08~~ | **DONE 0109d288.** `BlendOptions{decay 0.8, floor 0}` + `blend_with()`; ordering fixed to score-first. The floor is a parameter no surface sets yet — that is M64-01's per-surface config. |
| M69-09 | affinity as a stored 0..1 score per reader, recomputed nightly, replacing the per-work multiplier. |
| M69-10, M69-11 | Glicko-2 for the arena (deviation, volatility, divergence flag) and pairing that prefers similar rating with high deviation. The arena already stores per-match outcomes; this is a rating model swap plus a pairing policy. |
| ~~M69-12~~ | **DONE e93f94e6 on discovery.** `match_percent` + `reason` + `reason_code` per row; `SlotReason::sentence()` is the vocabulary. Draw, concierge and similar-to-shelf still emit bare ids — three call sites left. |
| M69-13, M69-14 | trust velocity and a live badge that is gained and lost, both derived from karma terms already stored. |
| M69-15 | probation above a configured population. Governance knob, default off. |
| M69-16 | three-layer content filtering (hardcoded, policy, taste) with reasoned overrides and an audit row. |
| M69-17, M69-18 | tag provenance (human/source/model/community, model may only propose) and trope clusters with a coverage checklist. Both are `work_tags` column additions plus write-path rules. |
| M69-20, M69-21, M69-22, M69-23 | the tag script, Saved Filters, the morning queue and import intake policy — one feature family: a Saved Filter is the object, and the other three are surfaces over it. Take M69-21 first. |
| M69-24 | one shared taste crate + JSON schema set with suki. A crate extraction; the estate rule says extract at the second caller, and suki is the second caller. |
| M69-26 | three-layer settings (platform default, operator override, reader personalisation) with bounds stated once. |
| M69-02..M69-07 | federation and preservation: peering by fingerprint similarity, signed taste passport, tombstones that survive re-import, mirrored works as visible attribution, hash-rescue ring, evacuation protocol. Each needs the shared taste crate (M69-24) and a signing key decision. |

## Batch 5 — the experiment platform (M64)

M64-01 (surface registry, 2–4 algorithms by config), M64-02 (append-only impressions), M64-03 (daily snapshots),
M64-04 (automatic promotion only on a sequential-test win with guardrails in bounds), M64-15 (console list with
hold/promote/preview) are one system and must be built in that order. The algorithms then plug in: M64-06
(tag_jaccard/tag_cooccurrence — exists, refused by name while sparse), M64-07 (crosspost_union), M64-08 (fandom_graph,
operator-managed adjacency), M64-09 (taste gravity, bounded, after the blend), M64-10 (length/status/rating/cadence as
first-class facets), M64-11 (co-shelving, reading lists first), M64-12 (co_read from session transitions), M64-13
(engagement_overlap with k-anonymity), M64-14 (embedding similarity, feature-gated on a configured AI provider).

## Batch 6 — the CLI (M63)

The `dh` surface is one binary with subcommands; these are independent and small relative to the engines:
M63-05b (`rec --link epub`, needs the download route), M63-10 (`lib sync --since`), M63-11 (`lib tag --from-csv`, rollback
file), M63-12 (`watch`), M63-15 (`doctor`), M63-18 (config profiles), M63-22 (`admin explain-rec`), M63-23
(`admin backup`/`health`), M63-24 (`bench`), M63-25 (`audit --since`), M63-26 (`stats`), M63-29 (`tui`), M63-30
(Calibre `metadata.db` as a source), plus catalogue rows M63-32 (fandom backfill), M63-37 (per-source engagement as
reported), M63-38 (per-operator quality weights).

## Batch 7 — extensions (M16)

M16-06 is the sandbox (`wasmi`, fuel, memory and output ceilings, host-call limits, bounded concurrency); M16-07 the
review workflow; M16-08 gallery mechanics; M16-11 the paid marketplace; M16-12 the acceptance list (the tests that make
06/07 real); M16-13 the theme model beyond one stylesheet; M16-14 operational rotation, rate limits and auto-disable.
Take M16-06 + M16-12 together: one without the other is a sandbox nobody can prove.

## Batch 8 — community, governance and growth (M45, M66, M29, M44)

M45-58 (erasure reaches the routing record) is the only M45 row that is a correctness obligation rather than a feature;
take it first. Then M45-39 (instance-death protection), M45-45 (taste as a versioned blendable object), M45-46
(preview-before-apply), M45-52/54/55 (notice-and-action, RPF knob, compliance tooling), M45-40/41/42/43/44 (curator
views, share attribution, claim-your-works, Discord bot, sister instances), M45-65 (identity gates see every spelling).

M66: GS-1..GS-5 are one feature (guest access with consent, the fold, the cookie pseudonym, trust from karma, juries)
and must be taken in order. M66-08 (LAN file sharing) is independent.

**M29-08 and M44-02 are the same product idea** (a community-ranked roadmap board). Implement once, under M44-02's Elo
board, and close M29-08 by pointing at it — a second implementation would be two sources of truth for the same list.

## Batch 9 — the remaining frontend (M71)

~~M71-06~~ (reasons shipped on the discovery feed, e93f94e6; rails take them as they migrate), M71-07 (console register), M71-08 (auto-hiding reader bar + typography panel), M71-09
(honest stale position), M71-11 (facet counts endpoint), M71-12 (ten-bucket histogram), M71-13 (coverage statements),
M71-14 (settings provenance), M71-15 (facet/query equivalence), M71-20  — M71-16 shipped as 4a7f8839 (sitemap + Audience::Anonymous)
(search, author, collection and series pages as documents — the last of ADR 0006's order, plus deleting the Svelte
routes of the pages already migrated).

## What is not in this file

Nothing is dropped. Four rows are `unsupported` for reasons already recorded in the ledger (M2-06, M6-10, M7-03,
M6-15), and six are `evaluated-and-rejected` with their reasons in the row.
