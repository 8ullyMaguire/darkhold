# The remaining roadmap, ordered for execution

`docs/requirements.csv` holds **66 planned rows** as of 2026-11-08 (10 of them the new M72 batch below). This file is the order they will be taken in and the
reason for that order, so a later session does not have to re-derive it. The ledger stays the source of truth: a row
moves when `scripts/ledger.py upsert` says so, not when this file says so.

Two rules that decide the order:

* **Things a reader can see come before things an operator can see, and both come before infrastructure.** A row that
  changes what a stranger can read or find is worth more than a row that changes what an operator can tune.
* **A row that gates other rows is taken as soon as its blocker clears.** Nine of the M64 rows read a snapshot built by
  M64-01..M64-03; taking them first would mean writing the same aggregation three times.

## Batch 0 — the family of products (M72, 10 rows) — take this first

Added 2026-11-08 at the user's direction, ahead of everything else, because it changes the *shape* of the work that
follows: the estate now runs four products on one host from one maker, and three of them have a frontend that shares
nothing with the other two. The survey, the decisions and the rejected alternatives are in
`docs/plans/estate-coherence.md`; the rows are `M72-01` … `M72-10` in the ledger.

The order inside the batch is a dependency chain, not a preference:

1. ~~**M72-01** publish `@polaris/ui` to the estate registry~~ **DONE** — 0.1.2 and inbox-client 0.1.1 published; a clone
   with no `estate/` sibling installs and builds. — today it is a `file:` tarball path that only resolves on
   this machine, so nothing else is adoptable until there is a version to depend on.
2. ~~**M72-02** one token vocabulary, palettes as data and a contrast gate over every palette~~ **DONE** — 0.2.0: six
   palettes, a written light ramp, per-palette literal CSS, 0 contrast failures; darkhold's three presets now come from
   it (browser-measured identical), and the gate caught a 1.8:1 control boundary in darkhold's own palette.
3. ~~**M72-03** the six primitives the estate has nowhere~~ **DONE** — @polaris/ui 0.3.1: checkbox/radio, avatar,
   breadcrumb, tooltip, kbd, action row; 11 render tests, six mutants red, adopted in WorkCard/Account/Docs/VoteBudget.
4. ~~**M72-04** one error envelope~~ **DONE on all four** — `{error:{code,message,request_id}}`, `code` UPPER_SNAKE,
   product extras allowed. Two classes escaped in this tree (an unknown `/api/` path and framework refusals — a 422 with an
   empty body was measured, then fixed); ficnexus keeps its legacy `err`/`msg` beside the envelope behind an env switch;
   ascend's frozen `CONTRACTS.md` was amended in the same commit as its code; kindred's middleware now puts the id in the body
   (its first test only exercised the helper and two mutants survived — the wiring is now tested through the router).
5. ~~**M72-05** one meta/health shape~~ **DONE and live** where it runs: `/api/v1/meta` (six named fields) and `/healthz`
   (`{status, version}`) on darkhold, ascend and kindred; `estate/scripts/meta-probe.mjs` exits 0 across the three, and the
   mutant the row named — `api_version` renamed in ascend — fails it by name. Two survey errors came out of probing: `:8050`
   is shortwave, not ficnexus (which has no process here), and ascend deploys from a non-git tree on the host.
6. **M72-06** adoption, darkhold → kindred → ascend, each proved by deleting the local copy.
7. **M72-07** the family nav module fed by `estate/generated/nav.json` — **done for darkhold** (`FamilyNav` in the site
   footer, three tests, three mutants, live-verified at https://darkhold.polarisocial.xyz/works); the sibling adoptions are
   one line each and belong with item 6 since both need `@polaris/ui` in those trees first.
8. **M72-08/10** the auth matrix and the noun/URL vocabulary — decisions plus their small code changes.
9. **M72-09** the design review as a gate — **done** (`gate.sh design`, 22 renders across three products, exit 0). It earned its place twice over: it is what proved ascend's `/fics` adoption renders, and it is what produced 14 contrast 'failures' on kindred that turned out to be the gate mis-measuring screen-reader labels.

**Batch 0 is complete: M72 10 of 10.** What the batch leaves behind is a family that shares a token vocabulary, one error envelope, one meta/health contract, one blurb order, one nav module, a written auth matrix, one noun vocabulary, and a gate that fails when any of it drifts.

What this batch is not: one skin. kindred reproduces AO3's visual grammar on purpose (`web/src/styles/kindred.css`
cites the OTW stylesheets) and ficnexus has no frontend at all (`docs/EXPORTER-SCOPE.md`). The family resemblance is
the grammar — component anatomy, spacing, focus treatment, motion budget, state language, URL shape — with each
product's own paper. A row that would restyle kindred into the Archive palette is out of scope and should be refused
in the ledger rather than half-done.

### A query nobody runs cannot fail to decode (exports.rs, 2026-11-08)

Closing `exports.rs` took three functions and turned up four defects, none of them in the code being added:

* `grants_for_export` selected `single_use` as INT4 into an `i64` field — PostgreSQL only.
* `bulk_items_for_export` had **no** PostgreSQL casts at all: three UUID columns and a `byte_size` INT4.
* `::text` on a `timestamptz` answers `2026-02-01 00:00:00+00` where SQLite's canonical TEXT answers
  `2026-02-01T00:00:00Z`. Same instant, different string: every RFC 3339 client parsed one engine and
  broke on the other. The grant columns are now rendered with `to_char(... AT TIME ZONE 'UTC', …)` so a
  read out of PostgreSQL matches the format the table stores.
* The fourth was not a bug but a duplicate: `owned()` in the route layer fetched an export and compared
  account ids in Rust while `find_export_for` did the same thing in SQL. It had never been called.
  `owned()` now *is* `find_export_for`, so the ownership rule exists once, in the place that cannot
  forget it.

The pattern holds across this whole ratchet: the orphan is never the bug. The orphan is the reason the
bug survived — a SELECT list, a cast, or a timestamp format is only exercised when something asks for
the row.

## Batch 1 — DONE

| row | what shipped |
|---|---|
| M71-10 | the reader's no-JavaScript guarantee as one test (`crates/app/tests/reader_page.rs::the_reader_needs_no_javascript_at_all`), plus the mutation that proves it bites. The position form it asserts already existed; the attempt to add one was a duplicate and was removed. |

## Batch 4 — ranking (M64 / M69)

M64-09 first half (71adfa72): gravity is a bounded mixture `(1-g)*base + g*taste` with a per-surface strength, a load-refused
ceiling and a published pull histogram; `discover.feed` and the concierge (`home.curated`, 1f05deff) have a candidate pipeline to pull on, and the reader's dial shipped in c9e4527d (`/api/v1/discovery/gravity`, narrowing only).

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
(`POST /operator/experiments/sweep?day=`, keyed on (surface, day)). Guardrails shipped in 17e84537: `conversion_rate`
(challenger ≥ 50% of the incumbent) and `traffic_share` derived per day from the snapshots, so demotion fires for real.
M64-15 read half 7e58e239
(`GET /operator/experiments`), write half 0612cdf7 (hold/release, promote and demote by hand, `source` on the record).
`preview-as-persona` remains: it is a reader-shaped view, not a write.

Batch 4 also has M64-10's first half (7e746375): `GET /api/v1/facets` answers length, status, rating and cadence from one
scan, reporting `unknown` beside `known` and a sentence instead of an empty histogram. It is embedded in `/api/v1/search` (with `facets_over` naming the set), `/api/v1/tags/{tag}` and
`/api/v1/fandoms/{fandom}` as of b870d6e4, which closes the row.

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

~~M16-06~~ shipped: `crates/sandbox` (`darkhold-sandbox`, `wasmi` 2) with one caller,
`POST /api/v1/operator/sandbox/run`. Every ceiling in the row is enforced and has a module that hits it: fuel
(a loop with no imports, which nothing else can stop), memory (`trap_on_grow_failure`, so a denied grow reports
`memory` rather than the module's own out-of-bounds trap), host calls, output bytes (a refused write appends
nothing), wall clock, per-call host-I/O budget, and concurrency (`Concurrency` on `AppState`, refusing with 429
rather than queueing). Revocation is an `AtomicBool` polled at every host call and once before entry. The honest
limits are in the row's notes: `wasmi` has no epoch interrupt, so a module that never calls the host is bounded
by fuel alone. M16-07 the review workflow; M16-08 gallery mechanics; M16-11 the paid marketplace; M16-12 the
acceptance list (the tests that make 06/07 real); M16-13 the theme model beyond one stylesheet; M16-14
operational rotation, rate limits and auto-disable. M16-12 is now the natural next step: the sandbox exists, so
the acceptance list has something to accept or refuse.

M16-12 partly shipped (`crates/app/tests/m16_12_acceptance.rs`): five of seven claims are now tested — infinite
loops, memory excess, host-API permission denial, uninstall removing grants, and reset reachable after a broken
theme. Two things came out of writing them: host APIs are now **default-deny at link time** (`Grant`, so an import
the manifest did not get is `permission_denied` before an instruction runs), and re-granting after an uninstall
used to be a `UNIQUE` violation, because the grant's primary key is `(account, manifest_id)` and revocation is a
column. Still open, and named in the ledger rather than glossed: the recipe half of "custom engines and recipes honour
opt-out" (no extension-supplied recipe or engine exists, and the only AI-provider path — M64-14 embeddings — is
not built, so `privacy::custom_processing_allowed` is enforced at `request_narration` and is the one predicate a
future recipe dispatch must call). Forking has since shipped: migration 0181 adds
`extension_manifests.derived_from`, `POST /extensions/{slug}/fork` keeps attribution in that column
beside an untouched document, enters `pending`, and starts with **no grants** — consent named the original id.
M16-12 stays `planned` on the recipe clause alone.

## Batch 8 — community, governance and growth (M45, M66, M29, M44)

M45-58 (erasure reaches the routing record) is the only M45 row that is a correctness obligation rather than a feature;
take it first. Then M45-39 (instance-death protection), M45-45 (taste as a versioned blendable object), M45-46
(preview-before-apply), M45-52/54/55 (notice-and-action, RPF knob, compliance tooling), M45-40/41/42/43 (curator
views, share attribution, claim-your-works, Discord bot), ~~M45-44~~ (sister instances, shipped 2026-11-08 — see the
section below), M45-65 (identity gates see every spelling).

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


M64-11 first half (ee413789): `co_shelving` is an engine over public shelves only (mutated predicate reddens the test);
following a list shipped in e62ab963 (0172_shelf_follows, refusals pinned by mutation); story.similar now reads its configured set and has a co-shelving arm (5dc85eae); library.because_you_shelved shipped its route (6bc3a869); story.also_enjoyed still has no route and no engine for co_read/engagement_overlap.

M69-21 first half (de8fb809): saved views take a per-account slug with PUT /saved-views/{id}/slug and GET
/saved-views/by-slug/{slug}; subscriptions, keybind and the blend half remain.

M69-16 layer two (cdeac9dc): content_policy_blocks with a required reason, revocation as a record, merged into
content_filter_sql::for_pseud so it reaches signed-out readers; per-work overrides remain.

M69-10 consumer half (48da17e2): volatility and divergence are carried from polaris-taste and gate ladder
eligibility through one is_settled() rule; the arithmetic stays in the service per ADR 0001.

M69-16 closed (cdeac9dc + b13fe04b): three layers — hardcoded SQL invariants, content_policy_blocks with a
required reason and recorded revocation, reader filters — plus per-work overrides that name the rule they
exempt and are rendered inside the single exclusion, so every surface inherits them.

M69-17 (ee606f1c): tags carry their claimant through the existing source/confirmation vocabulary — model=
inferred+unconfirmed is invisible to readers, a confirmation by a reader or wrangler reveals it, and an
inference cannot overwrite a person's row. Remaining: route every work_tags writer through assert_tag.

M69-20 (c6b253f9): tag scripts — a kept set of nodes applied to a work in one call, written through
tag_assertions so the tags carry a human claimant (author on your own work, curator on another's).
Batch apply and the browsing UI remain.

M69-18 (ed5f1f6e): trope clusters + coverage checklist. Advisory and read-only; the public route answers
404 until taxonomy.trope_coverage is switched on (decided OFF by default, noted in the ledger).

M64-06 (61f4da13): the refusal half shipped — a tag surface now refuses by name when tags and engagement
never meet, with the counts, instead of claiming to rank. Row stays planned on M63-32's backfill.

M63-32 (b387cb1e): tag-inference pass ships as proposals only (model claimant ⇒ invisible until
confirmed), with per-source coverage (items/tagged/with_likes/tagged_and_liked). The metadata backfill
still needs source category data the importer does not store.

M64-07 (f5471486): crosspost_union ranks a story once at the sum of its archives; feed wiring
proven by m64_07_crosspost_union (served once, first, 65 > 50).

2026-11-07 merge — thinkcentre f6bbb890 + 1b4797bf (W7 task 2 step 3: roles reach the authorisation
checks; the cross-site refusal and audit check mutation-proven) merged into master as 249a2bce.
Its migration came in as **0179_session_roles**, renumbered from 0178 because 0178_source_engagement
(M63-37) already occupied that number. Snapshot policy gained `sessions.roles` = drop_column
(2628 cols / 362 tables). The thinkcentre checkout is ff-merged to 249a2bce and clean.

Gate added alongside the merge (3cc2399e): `scripts/check-migration-numbers.py` in `gate.sh repo` refuses a repeated
number per engine, a number in one engine only, and an unnumbered file. Proven by planting each failure.

M64-08 (2e070f4f): the fandom adjacency map is configuration — [fandom_graph] version + weighted edges,
validated at load; db::fandom_graph::related_works orders by WEIGHT then likes and returns only
published/public works; GET /api/v1/fandoms/{fandom}/related publishes the map version, both neighbour
lists and the works. Row stays planned: tag.cross_fandom and author.similar have no route in this build,
so the map is not yet reached through the two surfaces the card names.

MERGED thinkcentre 3a32119d (W7 task 2 step 5 first batch: 4 routes classified, two admin doors that had no gate) as f1cb3ffe — no migration, no collision; route_inventory 5/5, admin_scope 1/1, clippy clean.

M71-12 backend (9fd5955a): GET /api/v1/facets/words returns 10 population-equal (quantile, not equal-width) buckets
over the same filtered set as /api/v1/facets, with low/high/count/share and measured/unknown/when_unknown; unmeasured
rows are excluded and stated, empty buckets dropped and renumbered. Row stays planned on the drawing control and on
M71-11's p95 measurement. M71-11's note corrected: /api/v1/facets has returned counts since M64-10, so its bookmark
("no route returns a facet count") was stale — the remaining work is the UI half and the measurement.

M71-13 (bf404997): db::taxonomy::dimension_coverage publishes works/catalogue/share/sentence in one statement; wired as
`coverage` on /api/v1/tags/{tag} and /api/v1/fandoms/{fandom} and into the document tag page (bare count replaced).
Zero-work and empty-catalogue cases have their own sentences. Row stays planned on collection coverage and on the
facet panel's leading line (UI; the API already publishes known/unknown/total).

M71-13 collections (a7664d54): events::collection_coverage counts entries vs readable entries INSIDE the collection
(published+public+undeleted), publishes hidden, three sentences (partial / all readable / empty). GET
/api/v1/collections/{id} carries `coverage`. Only the facet panel's leading line remains, which is UI — the API publishes
known/unknown/total per facet.

M71-09 (f3aca3fc): ResumeState (current/stale/unmeasured/conflict) with percent(): Option<u16> and display() '—';
positions_with_current_revision joins each stored row to its unit's current revision; /api/v1/reading/progress
publishes `resume`; save_progress now stamps the server's current revision when the client omits content_revision
(without that, a position could never be proven stale). Row stays planned on the reader bar/choice UI.

M71-14 (b799f828): settings provenance — GET /api/v1/settings/privacy publishes per-key {value, source, reset, note};
DELETE /api/v1/settings/privacy/{key}[?pseud_id=] clears one level (204/422/404 discipline). Migration 0180 adds
privacy_settings.origin because onboarding-written defaults were indistinguishable from choices and were reported as
the reader's own six decisions. Remaining: per-work level, deep links, and typography/gravity in the same vocabulary.

MERGED thinkcentre 9e65ab8b + 5375d879 (W7 task 2 step 5 batches 2-3: succession and retention move to the cross-site door, a fourth operator predicate dies) as c6d1a65f — no migration, no collision; route_inventory 5/5, admin_scope 1/1, clippy clean.

M71-15 (6c35aee9): the query grammar gained a third order — ValueKind::Rank — so `rating:<=mature` parses and renders as a
CASE rank; RATING_RANKS in query.rs is the single table (general<teen<mature<explicit). domain::filter_state holds the one state
{text, word window, ceiling, sort, pinned} and renders it twice: canonical query_string() and chips() (pinned ones neither
editable nor removable, with the reason) plus unexpressed_note(). GET /api/v1/search returns state{query,chips,unexpressed_note}.
Ideas rows M63-37/M63-38 removed because ideas_catalogue.py cannot run on this machine (two vault files absent).
Remaining: the panel UI, and provenance/source as real filters.

MERGED thinkcentre 2094ce7c (W7 task 2 step 5 batch 4: nine more operator predicates become one decision) as d2f95d57 — no migration, route_inventory 5/5, admin_scope 1/1, m71_15 4/4, clippy clean.

MERGED thinkcentre 2de6811a (W7 task 2 step 5 batch 5: the last config-gated door; refusals reach the modlog) as f7e237f5 — admin_scope 1/1, route_inventory 5/5, clippy clean. W7 task 2 step 5 complete.

MERGED thinkcentre 32a1b331 (W7 task 3 step 2: one-click envelope wired on the existing seam) as c8951a10 — new routes/admin_actions.rs, governance +66, server +7; route_inventory 5/5, admin_scope 1/1, m71_11 3/3, clippy clean.

M71-11 (41e847e4): facet counts measured, then cached. Pre-registered rule (82db6c0e): p50<=100 / p95<=200 on 50 sequential
requests -> FIRST RUN FAILED (p50 297 ms / p95 372 ms); EXPLAIN showed three seq scans + a disk sort, so no index helps.
crates/app/src/facets_cache.rs memoises per (database, filter set) for 60 s; responses publish computed_at_ms_ago and
cache_ttl_seconds. After deploy: warm p50 2.83 ms / p95 6.05 ms; cold 1,174 ms on the first request post-restart.
Same commit fixed two PG-only defects in the facet scan (SUM->NUMERIC decode, `?n` placeholders never rewritten) that made the
whole endpoint 500 on the live catalogue; crates/db/tests/facets_two_engines.rs is the guard. Remaining: the UI half.

## M16-07 — review workflow (implemented)

Stages live in `crates/domain/src/extension_review.rs` as a pure state machine; the routes in
`crates/app/src/routes/extension_review.rs` only store what it decided. Rules: no stage skipped, the submitter never
reviews, permission and security review are two different people, two distinct approvers publish, and a published
version is finished unless it is revoked.

Two things worth knowing:

* **Migration 0183** rebuilt `extension_manifests` with `PRIMARY KEY (id, version)`. Keyed on `id` alone, a second
  version was a UNIQUE violation — so "roll back to the previous published version" described a row that could not
  exist. The rebuild copies every existing row.
* **Automated checks are recorded, not dispatched.** `POST /extensions/reviews/{slug}/{version}/checks` takes the
  results an operator got from `/operator/sandbox/run`. Submit does not run the sandbox because it does not know where
  the package bytes live; when a package store exists, submit should call the runner and post its report.

Renewal-on-growth is evaluated at publish against each active grant's own capability set, which is why a 1.1.0 that
asks for `network` invalidates a 1.0.0 consent for `work.read` instead of inheriting it.

## M16-08 — gallery mechanics (implemented)

Installations and ratings in `crates/db/src/gallery.rs`; the rules in `crates/domain/src/gallery.rs`.

* **Idempotence is a statement, not a check.** `ON CONFLICT(id) DO UPDATE … WHERE extension_installations.uninstalled_at IS NOT NULL`
  gives three outcomes from one write: a new row, a silent no-op on an active row, and a reactivation after uninstall. The partial unique
  index on `(scope, manifest_id) WHERE uninstalled_at IS NULL` is what stops two concurrent installs from producing two active rows.
* **The count is a census.** `COUNT(*) WHERE uninstalled_at IS NULL` at read time. Reinstalling cannot inflate it, and it can go down.
* **The scope is the acting pseud**, taken from the session. A body field would let an account install as another identity.
* **An update never widens a grant silently**: the policy resolves the target, then the target's declared capabilities are compared with the
  grant, and the refusal names only what is new.
* **Licence compatibility is a rule about identity, not a lookup.** `CC-BY-SA-4.0` cannot be forked under `MIT`, and the classification is
  an explicit list because a prefix rule on `CC-BY` would put share-alike in the permissive bucket.

Two live PostgreSQL defects were found by `gallery_two_engines` (both engines green now): `pseuds.id` is a uuid so the review join needs
`::text`, and `stars` is `INT4` so it is cast to `BIGINT` to keep one decode type.

Not built: any UI for the gallery, and notifications on a new review — the routes are the mechanism, and a page is a separate row.

## M16-11 — the paid marketplace (implemented)

`crates/domain/src/payments.rs` (arithmetic) · `crates/db/src/payments.rs` (store) · `crates/app/src/routes/payments.rs` (doors).

* **Separate from credits, by construction.** Nothing here reads a credit balance and nothing in the credits tables reads this ledger. A
  credit balance must never be readable as a price, so the separation is structural rather than a rule in prose.
* **A purchase grants nothing.** It records a payment in `pending`; capture is the moment money exists, and the only moment the grant and
  the seller's share are written. With no provider configured, `Processor::Manual` returns `captured: false` and an operator settles it
  against a bank transfer — the honest behaviour, and the reason `pending` is a state readers see.
* **The split runs on the net of tax**, 85/15 with the remainder to the developer, so the halves sum to exactly what was charged.
* **Refunds are cumulative in one column** with `refunded_minor + amount <= amount_minor` in the statement itself. A full refund revokes the
  grant and reverses the seller's share; a partial one does neither.
* **Tax is inclusive and rounded half-up** from a stored profile, so `net + tax == gross`. Storing a profile is not registering for tax, and
  the response says so.
* **Money columns are BIGINT.** `INT4` overflows at ~21M euros; the two-engine test caught three `INT4` decode failures on PostgreSQL before
  the change, the same class the gallery row hit.

Gaps named in the ledger: no invoice document rendering, no processor webhook, and extension revenue settles through the existing
`author_earnings_ledger` → `request_payout` path rather than a separate payout ledger.

## M16-14 — webhook operations (implemented)

`crates/domain/src/webhook.rs` (grace window, `RateLimit`, `Breaker`) · `crates/db/src/webhooks.rs` (store) · `crates/app/src/routes/webhook_ops.rs` (doors) ·
`crates/app/src/webhook_delivery.rs` (the loop that enforces all three).

* **A rotation is a window, not a switch.** The old secret stays admissible for 24 hours because an instant rotation breaks every
  subscriber mid-deploy, and the failures it causes look like an outage rather than a configuration change. The header carries
  `v1=<current>,v0=<previous>` so a receiver needs no special case after rotating.
* **The rate limit counts deliveries, not attempts.** Throttled rows are excluded from the count — otherwise the limiter consumes its
  own budget and a burst extends its own silence — and each skip is *recorded*, because an absent delivery reads as an outage.
* **The breaker counts consecutive failures.** A subscriber that fails once a day for a year is flaky, not gone; twenty in a row is a
  receiver that has stopped answering. A success resets the count, but a success while switched off does not resurrect the endpoint:
  only an explicit, owner-scoped enable does.
* **The operator view omits secrets by construction.** An operator needs to know an endpoint is failing, not to be able to forge its
  deliveries.

Two tests run the real `deliver_notification` loop against a local receiver rather than asserting the arithmetic again — the mutant that
removed the limit check reddened exactly those. Remaining gaps (in the ledger): no retry backoff of its own (the outbox owns retries), no
per-event-type limit, and no notification to an owner whose subscription the breaker switched off.

## M16-13 — the theme model (implemented)

`crates/domain/src/theme_model.rs` (vocabulary, contrast, precedence) · `crates/db/src/theme_profiles.rs` (store) ·
`crates/app/src/routes/themes.rs` (doors) · `migrations/*/0187_theme_profiles.sql`.

* **A theme is data, not a document.** Ten tokens, a closed placement set, and `declarations()` emits custom properties and nothing
  else. That single decision is what makes "a theme cannot hide the safety controls" a property of the vocabulary: a theme has no
  selector, so it has nothing to aim at.
* **Slots cannot be removed.** A theme changes a placement from a closed list; a missing slot means *core*, never *gone*, and the
  renderer walks `Slot::all()`.
* **Scope is where the constraint lives.** An instance theme lays out the building; a reader theme recolours one room and keeps the
  building's layout. The check is in the model, so a future route cannot publish what `validate` refuses.
* **Publish is the gate, save is the form.** Both refuse a contrast failure, and publishing re-validates the stored row rather than
  trusting the save: a draft written when the rule was looser must not slip out later.
* **Reset deletes.** Writing the defaults would turn "reset" into a theme that drifts the next time the defaults change.
* **Safe mode beats the preview.** The preview is the theme most likely to be broken, so it is exactly what safe mode must override.

Gap, stated in the ledger: the model makes hiding *inexpressible*, but the renderer half is a contract, not a verified fact — no
frontend test asserts that the four protected surfaces sit outside the theme root, and there is no theme console UI.

## M16-12 — the acceptance list (implemented)

`crates/app/tests/m16_12_acceptance.rs` (one test per claim) · `crates/domain/src/recipe_document.rs` (the recipe schema and the
processing rule) · `crates/app/src/routes/extension_dispatch.rs` (the dispatch point).

The row closed on a decision rather than a feature: **"custom engines and recipes honour opt-out" has two answers, and they are not the
same answer.**

* An engine runs code or drives a process outside the host, so `custom_processing: refuse` governs it, and the refusal names the setting
  rather than the package.
* A declarative recipe composes strategies the instance already runs, so it hands nothing outside and consent does **not** govern it.
  Refusing an opted-out reader here would be a false positive that teaches readers the setting is noise.

That is structural, not conventional: a recipe's schema is closed (`stages[{strategy, weight}]`, weights in (0,1], at most eight, no
duplicate strategy), so a document that could carry an endpoint is refused *by name*. `processing(category)` is the one place that
answers which categories run, the route asks it before reading anything out of the body, and strategy names are validated against
`rec_engine::available_strategies()` rather than a second list that would drift.

Gaps named in the ledger: the blend does not read recipe documents yet (M69/M64 own that), so an applied recipe is validated and stored
but does not change the feed; and there is no engine runner to hand an engine package to.

### M69-09 — reader affinity (implemented 2026-11-07)

One stored 0..10000 score per reader with its three components (favourite overlap 0.5, tag similarity 0.3, arena
agreement 0.2), computed in a sweep that starts from the evidence tables. An absent signal is removed from both halves of
the mean instead of being counted as zero — a reader with no arena history is not "0.2 less aligned" — and a measured
zero is still a score. `evidence_bp` travels with `score_bp`.

The sweep is **operator-triggered** (`POST /operator/affinity/recompute`): there is no scheduler in this tree, so a
`tokio::interval` inside a handler would be a timer per request multiplied by the process count. The upsert means a
second run repairs rather than doubles, and a repeat over unchanged data reports zeros (a run that reports a change every
time is a sweep matching nothing). New evidence is absorbed by the next run.

Not wired yet: nothing consumes the score (trust speed, badges and membership are the intended consumers), so this is a
reading of evidence with a door, not a feature a stranger can see. The four PostgreSQL casts (`::uuid` on account and
work ids, `CAST(... AS TEXT)` on selects) are not decoration: the SQLite run passed while the PostgreSQL run failed on
three separate decodes before them.

### M69-11 — arena pairing (implemented 2026-11-07)

The round is chosen, not shuffled. A fandom group is sorted by rating and a window of four slides over it: **proximate**
minimises the span (a comparison that sharpens what the ladder already believes), **exploration** maximises it (a
comparison that locates a work), and both break an exact tie on the largest mean **deviation** — within an equally tight
window, the cards that can still move are the ones worth asking about.

The roll is passed into the domain function rather than drawn inside it, so "explores about epsilon of the time" is
testable: epsilon 0 never explores, 1 always does, and the route draws the roll from `rand` and takes epsilon from
`discovery.arena_epsilon` (refused at load outside 0..=1 rather than clamped — a probability outside its range is a typo,
and silently making it 0.15 leaves an instance believing it configured exploration).

**The calibration window is recorded, not applied.** A round names every card with fewer than five comparisons, a card
with no reading at all counts as inside it, and the ballot stores `calibrating` so a consumer can leave the vote out of the
global number without re-deriving which ballots were early — which it could not do anyway once more ballots arrive. Nothing
refuses the vote and nothing adjusts a rating here; the ladder arithmetic is `polaris-taste`'s (ADR 0001).

Gaps named: the deviation tie-break only breaks *exact* ties (a continuous cost was rejected because it makes the reason
sentence unquotable); no client renders `mode`/`calibrating` yet; `arena_appearances` counts best/worst only, because the
middle two cards of a round are never stored, so it is a floor rather than an exact count.

Two dialect facts paid for on the way: `arena_ballots.calibrating` is a BOOLEAN on PostgreSQL and INTEGER 0/1 on SQLite
(the repository's flag convention), which the PG campaign caught as `column "calibrating" is of type integer but
expression is of type boolean` — after the migration had been written IDENTICAL for both engines.

### M69-13 — trust velocity (implemented 2026-11-07)

The multiplier applies to the **score**, before the ladder is read: alignment factor `0.5 + affinity` (M69-09) and behaviour
factor `0.5 + Laplace-smoothed share of confirmed reports`, each **neutral at 1.0 when there is no evidence**, product
clamped to `[0.25, 2.5]`. The level written is `max(held, earned)`, so a multiplier can slow the next level and can never
take the one a reader holds — demotion stays the confirmed-report path, which has an appeal.

The basis carries the multiplier, both factors, and a sentence explaining the floor, so a support answer can name the
number. Off by default (`[trust] velocity`).

Honest limits: with today's factor ranges the product cannot leave `[0.25, 2.25]`, so the clamp is a guard proved on
`velocity_from_factors` rather than a participant; behaviour quality reads reports only, so it rewards good *reporting*,
not good judgement; and nothing renders the multiplier to a reader yet.

**A defect fell out of this row and is worth more than the row.** `days_since` parsed only RFC 3339, and PostgreSQL returns
`accounts.created_at` in its own rendering (`2026-03-02 20:38:18.441367+00`) — so on PostgreSQL the **tenure axis read
zero for every account**, silently, and every level on that engine omitted it. The parser now accepts both shapes and is
unit-tested against both; recorded against DM3-02. On the live instance the next recompute will raise levels for accounts
that were always entitled to them, which is a change to announce rather than to let happen.

### M69-14 — a live badge, gained and lost (implemented 2026-11-07)

Attuned is a **state**, so it cannot reuse `badge_award` (unique per account+badge, an event with a credit bonus). Two tables:
`live_badge_state` (what is held now) and `live_badge_events` (when it changed and why). Eligibility is two clauses —
`affinity >= 7500` **and** `measured weight >= 8000` — because M69-09 reweights over present signals, so a one-signal reading
can be 10,000 and a badge is a fact. The reason names the clause that failed.

A recompute that changes nothing writes no event; a change writes a row and enqueues `badge.gained` / `badge.lost` on the
outbox. `since` is cleared on loss. Events carry a `seq` so "newest first" is the order things happened in, not a tie-break.

Decided inside the affinity sweep (the requirement is "recomputed with affinity"; a second door is a second thing to forget).
Off by default (`[discovery] live_badges`): when off nothing is decided (no state row) and `/me/badges` answers 404, so a reader
cannot learn the instance measures them and hid it.

To enable: `[discovery]` → `live_badges = true`, then run `POST /api/v1/operator/affinity/recompute`.

Limits: no page renders it yet (frontend rows own that); the sweep is operator-triggered so a badge is as fresh as the last run;
only Attuned exists; a reader measured by exactly one signal is permanently ineligible by design.

### M69-02 — peering suggestions, with the hardcoded layer as a veto (implemented 2026-11-07)

Cosine over the published tag vector, the existing 0.25 floor, and — the half that was missing — a **veto**: a rule one side
declares and the other does not is a block, checked *before* any score is computed. Symmetric (a peer that enforces more is
also in collision), names the rule and the side enforcing it.

The layer is declared (`[discovery] enforced_rules`) and published in `/.well-known/taste-fingerprint.json` as `enforced`;
never inferred, for the same reason fandom adjacency is declared (M64-08) — a veto inferred from a sample can be outvoted by
a larger sample. A peer whose document predates the field collides with nothing, because an unanswered question is not an
answer.

Doors (both operator-only, both 404 unless `[discovery] peering_suggestions`):

    GET  /api/v1/operator/peering/suggestions   # ranked verdicts + reason per peer
    PUT  /api/v1/operator/peering/fingerprints  # record a peer's document (nothing fetches yet)

Order is the answer: suggestions, then vetoes, then near misses.

Limits: nothing fetches peer documents; the list does not write `federation_peers_v2`; the veto matches rule *names* only, so
differently-spelled rules collide spuriously — the fix is a controlled vocabulary, not a fuzzy match.

### M69-03 — a signed taste passport (implemented 2026-11-07)

ed25519 over canonical JSON (sorted keys, no whitespace, integers only in the signed surface — the tag vector is basis points,
because float formatting is the part of JCS that fails intermittently rather than loudly). Both doors sign/verify through one
function, so the round-trip test (export → import) is the real assertion.

The document is a **claim**: signed by the instance, not the reader. Its taste lands as `origin = 'imported'` taste signals, so
local evidence outvotes it as it accumulates. Unknown `version` refused rather than partially read; 180-day expiry checked in
both directions; caps on tags, sources and weights.

    GET    /api/v1/me/passport           # export, signed by this instance (needs [discovery] passport_key)
    POST   /api/v1/me/passport           # present one: verify, receipt, seed imported taste
    GET    /api/v1/me/passports          # receipts
    DELETE /api/v1/me/passport?origin=…  # receipt and the taste it seeded

Off by default (`[discovery] taste_passports`). The export carries canonical tag names (not local node ids) and only the
**public** key.

Found on the way: two renames the store had left behind — `chapter_revisions` → `revisions` (0122) and `taxonomy_nodes` →
`canonical_entities` (0124). Both queries compiled green and would have failed on any live instance.

Limits: nothing fetches a peer's passport; no UI; not linked to polaris-id, so "same person" rests on the reader presenting it;
`favourite_sources` is exported but unconsumed.

### M69-04 — a tombstone that survives re-import (implemented 2026-11-07)

Keyed by **work identity**, not URL: title + author + language, normalised (NFKD, lowercase, marks dropped, punctuation and
spacing collapsed) and hashed. A takedown that stops one string stops one import; the work comes back from a mirror
tomorrow, which is the failure a takedown exists to prevent. The body is not the key — imports arrive without one, a body
legitimately changes, and two mirrors rarely match byte for byte. `text_key` exists for a later second key, unused today.

Normalisation is deliberately conservative: "The Storm, part 2" is a different work from "The Storm" (an over-matching
tombstone removes a sequel nobody complained about), and an empty author is not a wildcard. A retitled copy escapes — the
cheaper failure.

The refusal is checked **before the library row is written**, in both the URL import and the bundle restore, so a refused
import leaves no item and no units. The queue row is `failed` with code `tombstoned` and a message citing the tombstone id,
its reason and its filing date — permanent, so the worker does not keep asking.

Every refusal increments `refusals` / `last_refused_at`: a tombstone with a zero is a mistyped title, and that is the only
way an operator can tell.

Withdrawal writes `withdrawn_at` + reason rather than deleting, because the row is the answer to "why did my work vanish?".
Re-filing re-arms it and keeps the paperwork reference.

    POST /api/v1/admin/tombstones                 # file by identity; the response echoes the key it derived
    GET  /api/v1/admin/tombstones                 # withdrawn ones included, with refusal counts
    POST /api/v1/admin/tombstones/{id}/withdraw   # needs a reason

Off by default (`[discovery] work_tombstones`), read at the point of refusal. Doors answer 404 while off, 403 for a
non-operator while on.

Found on the way: dropping combining marks **without decomposing** split `Á` from `A`+acute into two keys — caught by the
accent test; NFKD before lowercasing is the fix.

### M69-05 — mirroring by taste, attributed on the page (implemented 2026-11-07)

The reader-visible half of instance-death protection (M45-39 is the paperwork). Who may hold a copy is **computed** from the
fingerprints peers publish against this instance's own: `peering::assess` decides, so a collision veto beats a high score and
is reported as the rule with `similarity: null` — a veto is not a low score.

The mirror floor is **0.5**, twice the suggestion floor: a suggestion says a reader might like it there, a mirror commits
somebody's work to another host's keeping. `mirror_floor` in config can raise it, never lower it. Mutation testing caught
that the first cut of tests only exercised peers below 0.25, so deleting the mirror check left the suite green; a peer at
0.372 now holds the gap under test.

    GET  /api/v1/operator/mirroring/eligible            # who may hold a copy, and why the others may not
    POST /api/v1/operator/mirroring/{work}/mirrors      # record one (refused with the number if not eligible)
    POST /api/v1/operator/mirroring/mirrors/{id}/retire # lost or withdrawn, reason required
    GET  /api/v1/works/{work}/mirrors                   # the sentence a work page prints

One row per (work, peer): re-recording replaces, and re-recording after a retirement re-arms it. The similarity stored is the
one the decision was made on. Retirement keeps the row — a reader who saw the line yesterday is owed an explanation.

The public door is anonymous and narrower than the operator one: published, public, undeleted works only (404 for a draft).
A closed door answers 404 to *every* request, bodies parsed after the switch check — an extractor running first answers 422
and tells a caller the route exists.

Off by default (`[discovery] work_mirroring`).

Limits: nothing transfers bytes (no peer API client), nothing verifies the peer still serves it, and the SPA does not render
the sentence yet — the door returns it.

### M69-06 — the perceptual-hash rescue ring (implemented 2026-11-07)

Two hashes, two claims. The **content hash** is identity: a survivor reporting the same bytes may be credited without a human.
The **perceptual hash** is evidence — it survives re-encoding, which is why it finds a copy and why it cannot prove one; the
failure mode is a work's page showing somebody else's picture. So a perceptual match is a proposal, never an attachment:
nothing here writes availability links, because attaching is `media_resilience`'s audited path.

A tie is an answer: two equally close survivors (or two holding identical bytes) produce `Ambiguous` with both names and **no
stored match**; `accept` then requires the operator to name one. Dimensions are a second lock — a candidate more than 1.25×
off in either axis is excluded, because a thumbnail beside its original is a legitimate pair and the wrong rescue — while an
*unknown* dimension never disqualifies.

    POST /api/v1/operator/rescue/copies            # a survivor says what it holds
    GET  /api/v1/operator/rescue/copies
    POST /api/v1/operator/rescue                   # {dead_peer, media_reference_id} → outcome
    GET  /api/v1/operator/rescue                   # every hunt, settled ones included
    POST /api/v1/operator/rescue/hunts/{id}/resolve

Every hunt is a row, `not_found` included — "we looked and nobody had it" is the answer an operator needs most. The outcome
and the threshold travel with it; a later copy must not rewrite what the operator was told.

Two defects found by doing it: the dead peer was first a **path** segment (`https://…` is two segments to the router, so
every hunt 404'd), and `$n::timestamptz` binds into TEXT columns rewrote the stored stamp into PostgreSQL's rendering.

Off by default (`[discovery] media_rescue`).

Limits: nothing fetches a survivor's copy (no peer API client) — the ring matches claims; `accept` records who has it, the
transfer is the operator's next step; hunts read at most 500 recorded copies; nothing decides a peer is dead automatically.

### M69-07 — the evacuation plan and its receipt (implemented 2026-11-07)

An instance announcing it is going away hands its works to taste-similar peers and writes down **who took what**. The plan is
a pure function (`domain::evacuation::plan`): works in, ranked eligible recipients in, placements out. Deterministic — the same
inputs give the same plan on every run, or two operators disagree about who took what.

**Balance is a rule, not a preference.** Filling every placement with the closest peer is the naive plan and a bad one: the
peer closest to *everything* takes everything, and an evacuation that concentrates a whole catalogue on one host has recreated
the single point of failure it was performed to avoid. So the plan fills the least-loaded peer *inside* a 0.15 similarity
window first, and only outside that window does taste win.

**The fork is the failure.** `UNIQUE (notice_id, work_id)` stops the plan placing one work twice. The receipt's job is the
other half: a second evacuation — an instance that closed and announced again, or one revived under a new operator — cannot
hand a work to a different peer once someone accepted it. The refusal names the peer *and* the notice.

    POST /api/v1/operator/evacuation                        {reason, note?, limit?}
    GET  /api/v1/operator/evacuation                        notice + assignments + receipt + forks
    POST /api/v1/operator/evacuation/assignments/{id}/settle {state: accepted|declined, note?}
    POST /api/v1/operator/evacuation/close                  {state: withdrawn|completed}

Recipients are **computed** (`mirroring::eligible_peers`, same floor as mirrors), never typed: an operator picking hosts by
hand is how a shutdown becomes a copy of somebody's catalogue on a host nobody vetted. With no eligible peer the announcement
is **refused** rather than planned empty — a receipt of zero looks like the work was placed.

Off by default (`[discovery] evacuation`).

Limits, named honestly: (1) nothing transfers bytes — `settle` records what a peer said, and the transfer is the operator's
next step through existing machinery; (2) no peer authenticates a claim, so a recipient that says "we have it" and does not is
indistinguishable here from one that does; (3) the plan reads at most 1000 works per announcement and ignores works that are
not `published` + `public`; (4) nothing notices a shutdown automatically — an operator announces it; (5) a second acceptance
answers 422, not 409, because this error vocabulary's only conflict variant names two version numbers this refusal does not
have, and inventing a version pair to reach a 409 would be a lie in the payload.

### M69-10 — Glicko-2 for the arena, and the arena nobody could reach (implemented 2026-11-07)

Elo has no notion of confidence: a card with two ballots and a card with two hundred sit on the same ladder as if their numbers
meant the same thing. Glicko-2 carries a rating, a deviation and a volatility, so "provisional" becomes a number.

`domain/src/glicko.rs` is Glickman's algorithm, steps 1–8, reproducing the paper's worked example (1464.06 / 151.52 / 0.05999)
to within 0.05. 1500-mean scale preserved, so nothing a reader saw changes meaning. A card enters at its Elo rating with
**maximum** deviation — an Elo number is a position without an uncertainty — and the deviation then shrinks by being compared
and grows with silence (additive in variance: reaching the ceiling from deviation 60 takes about 1200 updates at σ = 0.06).

**The divergence flag was re-derived once, in the open.** The first rule was "a ballot that moves the rating further than
twice its own deviation"; a measurement killed it — a settled card (deviation 40) beating an opponent 600 points above moves
**26** points against an 80-point bar, because Glicko-2's rating step is bounded by the squared posterior deviation, not by the
surprise. A rule that cannot fire on a four-card ballot is decoration. The flag now means what an operator can act on: this
ballot **inverted a gap of at least 300 rating points**.

**Two defects in the Glicko-2 arithmetic, both caught by the paper's example:** the volatility step returns `e^(x/2)`, not
`e^x` (returning the square of the volatility makes every number look plausible and the deviation barely grow), and the
`k` counter in the Illinois bracket is an `f64`.

**The real find:** `GET /api/v1/roadmap/arena` and `POST /api/v1/roadmap/suggest` had **never worked over HTTP**. They lived in
`roadmap::router()`, nested into the outer router at `/api/v1` — and a router keeps only the last `nest` for a prefix, so the
`.nest("/api/v1", api)` further down silently discarded them, while `read_router()`'s public board (merged at the root) worked
next door. The store tests passed, the route table listed the paths, and a reader got a bodiless 404 from a surface that was
"mounted". Fixed by putting both routers in `api`, the router that is actually nested, and the new test requests the arena
through HTTP rather than trusting the table.

    [roadmap] glicko = true    # off by default: a rating plus an uncertainty is a claim about the board

Limits: the arena still pairs candidates by Elo and match count — "prefer uncertain pairs" is available from the deviation now
but not wired into `arena_candidates`; `diverged` describes the last ballot that moved a card, not its history; and the
federation ladder's numbers remain `polaris-taste`'s under ADR 0001 (M69-10's consumer half, `db::ladder::is_settled`).

### M69-22 — the morning queue and the daily surprise (implemented 2026-11-07)

Two shapes, two jobs, and neither is a new surface: both are drawn from a **saved filter**, so the operator retunes the queue
without a deploy and the response says which filter answered.

    GET /api/v1/discovery/morning
    → { day, filter: {slug, name, params, url}, queue: [...], surprise: {...} }

**The queue is diverse by rule, not by hope.** Five excellent works from one corner of the archive read as one recommendation
repeated. So the selection is a round — first from each fandom, then the second from each, stopping at two per fandom — which
keeps the filter's own ranking *inside* each bucket and still puts the pool's best work first. Round-robin is over
first-appearance order, so the result is deterministic; untagged works share one `unknown` bucket, because otherwise a
catalogue of untagged rows would look perfectly diverse. A pool too small to fill the queue is filled rather than padded: the
cap governs a *diverse* choice, not a short one.

**The surprise is stable for the day and different between readers.** Seeded by FNV-1a over (day, reader), not `DefaultHasher`
(which is explicitly not stable across Rust releases) and not `rand`. A surprise that changes on reload is a slot machine, and
one that is the same for everyone is not a surprise.

**Nothing here invents a filter.** The queue names a slug on the operator's account (`[discovery] morning_queue_view`); off,
unnamed, or pointing at a filter that does not exist all answer **404** — an empty 200 would say "we looked and found nothing"
when the truth is "nobody configured this". A **private** filter is refused with the reason, because serving it would publish the
filter's contents to every reader, and a **library**-scoped view is refused too: the queue runs a public catalogue filter.

Missing piece named: M69-23 (a saved filter as an import's intake policy) and M69-21's public life (subscriptions, keybinds,
blends) are separate rows; this one is the reading surface.

### M69-21 — a saved filter's public life: the keybind, the follow, the blend (implemented 2026-11-08)

The filter already existed (0009) and already had a slug (0173). What was missing was every way a reader *uses* one without
rebuilding it, and all three additions are relations to that object rather than a second storage shape for a filter:

    PATCH  /api/v1/saved-views/{id}/keybind   {keybind: 1..9 | null}
    GET    /api/v1/me/keybinds
    POST   /api/v1/saved-views/{id}/follow     DELETE the same path
    GET    /api/v1/me/followed-filters
    GET    /api/v1/saved-view-blends           POST the same path
    GET    /api/v1/saved-view-blends/{id}/items   (the blend, answered)

**Digits 1–9 and nothing else.** `0` is the key beside `backspace`: a binding there would be pressed by accident more often than
on purpose, and ten is not a number key. The CHECK lives in the migration as well as the route, because the route is not the
only writer a script has.

**A collision is refused, not resolved.** Two filters on one digit means the reader presses one key and lands on a surface they
did not expect, and the second filter is unreachable in a way they cannot see. The refusal names the digit so the choice is
theirs.

**Following is one row per (reader, view)**, and it is deliberately *not* `search_alerts`: an alert belongs to the pseud that
owns the view, and a subscription belongs to the reader who asked. A private filter cannot be followed — otherwise the owner's
next edit publishes into a stranger's feed — and that refusal is in the store as well as the door.

**A blend is two filters answering one question.** `interleave` alternates by position, keeping each side's own order;
`union` appends with the left-hand position winning a duplicate. Nothing is re-ranked: a blend that reordered a side would be a
third ranking, and the reader who built each filter would not recognise their own results. A third mode (weighted) was
considered and rejected — a weight is a ranking question dressed as a filter, and one filter with both conditions already
expresses it. Both sides must be the caller's own filters: naming somebody else's private view would be a read of it through a
new door.

Off by default (`[discovery] saved_filters`), so every door answers 404 on an instance that has not chosen it — and the switch
is on the `library` module, because a saved filter *is* a library object.

### M45-44 — sister instances: publish, subscribe, and a directory by topic (implemented 2026-11-08)

Four surfaces, one vocabulary. `GET /federation/picks` publishes an ActivityStreams `OrderedCollection`; an operator names a
peer's collection at `PUT /operator/federation/picks`; the worker's maintenance pass fetches; `GET /federation/peer-picks` shows
what came back; `GET /federation/directory?topic=` finds instances by the topics they publish.

**A pick is a public catalogue row at Aggregate order, and the alternatives lost for different reasons.** Operator-curated picks
would need an editorial surface that does not exist (§16.11), so the federation feature would depend on an unbuilt admin UI.
"This instance's own works" is nearly empty on an instance that imports more than it publishes — a peer subscribing would get
nothing, which is the failure mode that looks like a working feature. The anonymous discovery feed is ordered by trend and
freshness, which changes hourly: a polling peer would read every fetch as a new set of picks. Aggregate order is the catalogue's
own default, so the collection is stable between fetches and inherits the catalogue's eligibility for free (M71-16) — a
restricted work cannot appear in the collection for a different reason than it cannot appear on the page.

**A fetch replaces, it does not accumulate.** The collection is the peer's *current* list. A work they stopped recommending must
stop being shown here, or the surface becomes an archive of what a peer once liked and a reader cannot tell which is which. The
delete and the inserts are one transaction, so a failure mid-write cannot leave a peer with no picks at all.

**Every failure path records.** The first version recorded only the failures that got as far as an HTTP status, so a host that
does not resolve — the case an operator most needs to see — left the row saying "1 picks" from a fetch that had worked an hour
earlier. The test caught it. Now `last_status` and `last_error_at` are written on the transport error, the non-2xx, the parse
failure and the store failure, and a failing fetch changes no picks: an unreachable peer is not evidence that its list is empty.

**A failing peer still advances `last_fetched_at`.** The sweep orders by least recently *attempted*, not least recently
succeeded: a peer that never advances would be retried every pass, at the head of the queue, forever.

**The subscription URL must be the host it is filed under, and must be https.** A subscription is a request made on the
operator's behalf; one filed under host A while pointing at host B is a way to make this instance fetch somewhere the operator
did not say they were fetching. Loopback http is refused at the door too, which is why the fetch test seeds its subscription
through the store — the door's rule is proven by its own assertions, and the test is about what happens to a peer's document.

**Re-pointing is an update, not a second row.** The host is the identity: two feeds from one host would show every pick twice and
no reader could tell the rows apart. Disabling is `enabled = 0`, kept rather than deleted, so turning a peer back on does not lose
when it was first trusted — and `recent_picks` joins the subscription so a disabled peer's picks stop being shown without being
destroyed.

**A pick is never reconciled with a local work.** A pick that matches something in this library is still shown as the peer's, by
URL. Attributing a pick is the point; guessing an identity match would turn "they recommend this" into "we think this is the same
thing", which is M69-05's question asked badly.

**The directory matches whole topics, case-insensitively, and omits what it does not have.** A substring match would make `fluff`
find every `fluffy` instance, turning a directory into a guess. Instances that do not name the topic are omitted rather than
returned at weight 0.0 — "we have nothing on this" and "we have a little" are different answers, and only one is a hit (a mutant
that scored a miss 0.0 was caught by this). Each hit carries `topics_named`, because a weight of 0.4 means one thing on a
three-topic instance and another on an eighty-topic one. Private themes are never listed; a topic nobody names is an empty list
rather than the whole directory; a search with no topic is a 422 rather than a silent "everything".

Proof: `crates/app/tests/m45_44_peer_picks.rs` — three tests on SQLite and PostgreSQL, the fetch one against a **real loopback
HTTP server** (a mocked client proves the parser was called, not that a collection a peer actually serves is read). Six mutants
red: replace→merge (the second fetch hits `UNIQUE (subscription_id, work_url)`), `recent_picks` ignoring `enabled`, the host-match
rule off (200 for a URL on another host), the dedupe off, a missing topic scored 0.0 instead of omitted, and private themes in the
public directory. `route_inventory` 5, `migration_catalogue` 6, clippy clean, orphan baseline unchanged at 181.

### The ratchet found a missing half, not a missing door (federation.rs, 2026-11-08)

`crates/db/src/federation.rs` holds 8 orphans and they are not a surface gap like the coordinates and batch-window
modules were. Reading them before wiring produced the reason: **darkhold federates in, and has never federated out.**

- `ap_inbox` verifies signatures and stores activities; `ap_actor`/`ap_outbox` answer remote servers. That half exists.
- Nothing *sends*. `federation_queue` has one writer — `enqueue_activity`, itself orphaned — so nothing ever enqueues;
  `get_pending_queue`/`mark_queue_sent`/`mark_queue_failed` have nothing to drain.
- The two halves the sender would need are also missing: there is **no RSA signer** for outbound HTTP Signatures
  (`http_signature.rs` verifies and builds signing strings; `keys.rs` loads a public PEM and can generate a pair, but
  nothing signs), and there is **no reader for `ap_activities`** by id — the queue stores `activity_id` and the payload
  is never read back.

Building five admin doors over that would have been the most convincing possible fake: green tests over a queue that
cannot fill, marked sent by a drain that cannot sign, verified by a test that only ever talks to itself. The honest end
is a row with a spec — outbound AP delivery (signer, payload read, drain with the SSRF and backoff rules
`webhook_sender` already implements, and one real enqueue path: outbound Follow) — and it is recorded here so the next
pass starts from the finding instead of re-deriving it.

### The public door is a projection, not the row (library.rs, 2026-11-08)

`public_bookmarks_for` is the only query in `library` that takes no `account_id`, and its doc comment says
why: it serves other readers, and `is_public` is what makes that safe. What the comment does not say — and
what the door has to decide — is that the *row* is not the answer. A `Bookmark` carries the owner's account
id and their private note. A bookmark being public publishes neither, so `GET /works/{id}/bookmarks/public`
answers `{unit_id, position_permille, created_at}` and a count, and the bookmark's own id is left out too: a
stable row id is a capability nobody granted.

The companion read, `GET /library/items/{id}/updates`, is scoped by **account**, not by item — one reader's
check history is not another's, and the honest answer for an item you have never had checked is `null`
rather than an empty report.

One correction belongs here because it is the trap in this file's own neighbours: `update_checks.checked_at`
is **TEXT** in the PostgreSQL schema (`0009_library.sql`), unlike `download_grants.expires_at` and the
`export_jobs` timestamps, which are `timestamptz`. Wrapping it in `to_char(... AT TIME ZONE 'UTC', ...)` — the
fix that *was* right one module over — turned every item view into a 500. Cast-or-render is per column, and
the migration, not the previous module, is the authority.

### The catalogue was never recorded (imports.rs, 2026-11-08)

`upsert_source`'s doc comment, written when the function was, says: *"Called at startup for every registered
adapter, so the catalogue a reader sees cannot drift from the adapters the binary actually has."* Nothing
called it. The `sources` table was whatever a migration had seeded, so a deploy that added an adapter showed a
source nobody could pick, and one that removed an adapter left a row pointing at a parser that no longer
existed. `serve` calls `source_sync::sync_source_catalogue` now, straight after `AppState::new`, before the
listener binds.

Two things it deliberately does not do. It does not touch `enabled`, so a paused source stays paused across a
deploy, which is `upsert_source`'s other half. And it does not fail the boot: a source that cannot be written is
logged and skipped, because a stale catalogue row is a better trade than a server that will not start. The
adapter version recorded is the build id — a source's capabilities are its parser's, and the parser ships in
the binary, so "which build wrote this row" is the only version string that means anything.

**Two of the six have no live producer, and that is a finding, not a gap left open.** `list_credential_events`
and `pseuds_with_unconsented_credentials` read `source_credentials`, and every adapter in
`crates/scrapers/src/sites` declares `authentication: AuthKind::None` — no shipped source needs a login, so no
credential row can be created through the API at all. They have their doors and are exercised against rows the
store writes (`m70_13_imports_catalogue`), because the tables and the audit trail exist and a door is the only
way their SQL is ever decoded; the honest statement is that the doors are tested, not that they are reachable
live. The first authenticated source is what turns them on, and the same pass that adds it will find whatever
those two queries get wrong — as every other first caller in this ratchet has.

One limitation is worth naming because the test had to work around it: `credential_audit_events.occurred_at` is
second-resolution and the ordering tiebreak is a random UUID, so two events recorded inside the same second come
back in an arbitrary order. The test moves one row back a day to make "newest first" a real claim. A trail that
cannot order two events in the same second is not wrong, but it is not an audit trail either — a monotonic
sequence number belongs in that table the first time anyone needs to prove an order.

### A queue with no producer reads like a healthy instance (rating_integrity.rs, 2026-11-08)

`GET /admin/anomalies` answered `{count: n, items: []}`. It counted the queue and listed nothing, so an operator could
learn that seven anomalies existed and never learn which works they belonged to — and `insert_rating_anomaly_event`,
the one function that could have filled the queue, had no caller at all. "0 anomalies" from a detector that never ran
is indistinguishable from "0 anomalies" from a clean instance, which is the worst property a moderation queue can have.

The producer is `rating_watch::detect_bursts`, one rule: a work whose recent public ratings (six-hour window) came
from freshly created accounts (younger than seven days), at a threshold of five. Severity is the observation — eleven
fresh accounts is a stronger signal than five, and a constant would make the two identical to whoever reads the queue.
The detail carries the rule and its parameters, so a curator can argue with the numbers rather than the verdict.

Three refusals are deliberate, and each is written beside the code:

* **It never marks a work contested.** §33.2 gives that to a quorum of curators. A detector that can hide a work's
  aggregate by itself is a machine moderator, whatever its threshold.
* **It never duplicates an event.** A second run inside the same brigade finds the same works and records nothing, or
  severity would measure how often somebody pressed the button.
* **It is a button, not a timer.** There is no scheduler in this binary for it to ride; the scheduled form is a
  follow-up with a name, not a claim this file makes.

The mark itself got its doors (`POST`/`DELETE /admin/works/{id}/contested`, reason required) and its *point*: the
public `rating-summary` now refuses to publish a contested work's score and says `suppressed: "contested"` instead of
answering a null. §33.2 asks for both halves — never published as a public score, and an aggregate display that states
when a guard is suppressing a number — and a silent null would have lied in the second half: "nobody has rated this"
is a different and untrue claim about a brigaded work with five ratings.

One mistake worth keeping: the first version wrote `kind = 'fresh_account_burst'` and migration 0019's CHECK rejected
every insert. The schema is the authority on the vocabulary of a column; the rule's name belongs in `detail`.

**Deploy is batched, not skipped.** This pass adds four live admin routes and changes what `/admin/anomalies` answers,
but each thinkcentre build costs ~16 minutes and a named backup. The rows since M45-14 (coordinates, leakage, devices,
library reads, exports, imports, ratings) ship in one deploy with one `pg_dump` and one verification pass at the end of
Batch 1, rather than six builds nobody reads the output of.

### A job id was a capability (translation.rs, 2026-11-08)

Five store functions had no caller — `units_for_job`, `upsert_unit`, `record_publication`, `add_memory`,
`lookup_memory` — and reading them for the doors turned up a sixth problem that was not an orphan at all:
`GET /translations/{job_id}` answered **anybody who had the id**, with no ownership check anywhere. A
translation job carries the provider, the quote transaction and the language pair of somebody's work. It is
owner-scoped now and answers 404 to a stranger (ADR 0030), and the four new doors all go through one
`owned_job` helper so the refusal cannot drift between them.

The units are per paragraph, and that is the point of the read: one job can have three paragraphs approved and
seven pending, and a caller who only sees the job's state cannot tell. The upsert is keyed on `(job, unit,
paragraph)`, so recording the same paragraph twice updates it rather than appending a second translation of the
same sentence. A paragraph cannot be `translated` with no target text — the state is a claim about the text
beside it, and accepting the claim without the text puts a paragraph in the reviewed pile that nobody can
review.

Publication is gated on `approved` and the order is load-bearing: the job transitions first, the publication
row second. A publication row for a job that never moved is a claim about an event that did not happen.

Translation memory got its write and its match beside the existing list. Both are keyed by `memory_key`, one
SHA-256 of the **trimmed** source sentence computed in the app rather than accepted from the client: a client
that normalised differently — trimmed, lowercased, collapsed whitespace — would store a pair its own lookup
could never find, and the failure would read as "memory is not working" rather than "two hashings". The lookup
therefore takes the *sentence*, not the hash, so there is exactly one place that decides what a key is.

**A note on mutation discipline.** The unscoped-job mutant survived its first run, and the reason was not a
test gap: the predicate is duplicated — once inlined in the handler, once in the new `owned_job` helper — and
`replace(..., 1)` rewrote the helper while the test exercises the handler. Re-targeted at the handler it reddens
(200 vs 404). That trap is now recorded in `mutation-gate-discipline/references/mutation-survivor-triage.md`,
because it is the second time in this session a survivor was a misplaced edit rather than a blind test.

### Nobody could import a history (taste_import.rs, 2026-11-08)

The module with the longest header in the crate had no route and no mount: nothing outside `crates/db/src/taste_import.rs`
named it, so `import_signals` — the producer — had no caller, and a reader who arrived with five years of bookmarks could
not bring them. Unlike the earlier modules, the tests were not the problem: the store had five tested functions and no
surface at all.

Four doors later, §49.6–§49.8 are enforced at the door as well as in the store. The origin is **not a parameter** of
`import_signals`, so a door cannot mint an organic signal even by mistake; `origins` reports both the row counts and the
discounted weight because "how many" and "how much" are different questions with different answers — twenty imported
bookmarks and ten ratings given today are thirty rows and twenty-five units of evidence (§49.6 fixes the direction of
the discount and leaves the magnitude to us; the constant is now returned so the number is explainable without reading
the crate).

**A module decision, made rather than inherited.** These doors could have hung off `tasting`, which is the §49 section's
own module — but `tasting` is opt-in by design (`PerUser`, default off), and putting the import behind it would mean a
reader cannot bring their bookmarks unless they first answer questions about their taste. Importing a history you already
have is data portability, not calibration. So `taste_import` is its own module: `default_on: true`, `Toggle::Operator`,
`RouteClass::Write` (an import moves the reader's weights, so the limiter's class has to be the one that costs), and
`STABLE_ROWS` went 82 → 83 with the reason written above the constant.

**The mutant that survived for the right reason.** Dropping the credential-ownership check left the suite green, and that
was not a misplaced edit: the test named an all-zero credential id, so "not yours" and "does not exist" were the same
answer and the ownership filter was never the thing under test. Replaced with a sealed credential belonging to another
reader — plus the control that its owner *may* use it, which is what makes the refusal an ownership check rather than an
existence check — the mutant reddens (200 vs 422). Recorded because the lesson generalises: a negative test that cannot
distinguish its own two failure modes proves only that something failed.

Ratchet 136 -> 131.

### A table nothing could fill (taste_health.rs, 2026-11-08)

Five functions and no caller: the per-work vector's producer, its writer, its reader, and the two halves of the taste
probe. The damage was a table nothing could fill — `work_taste_vectors` was read by `get_work_vector` and written by
functions that nothing called, so every read answered `[]`. A cached vector that is never written reads exactly like a
work nobody has taste in common with, and no error anywhere distinguishes them.

Four doors, and the shape of the answer carries most of the decision. `[]` meant three different things and the door now
says which: `unavailable` (this instance has no admin taste profile, so no work has a vector — a configuration fact),
`computed` (there was a profile and no stored vector, so this call derived and stored one), `cached`. A bare `[]` would be
a claim about the work that nobody made.

**Serving a stale vector is a decision, written down rather than hidden.** A tag edit does not touch
`work_taste_vectors`; recomputing on every read makes a hot work pay for its popularity; and there is no hook in the tag
editor that would let the vector be invalidated where the tags change. So the read serves what is stored, `POST` on the
same path is the only refresh, and the test asserts the staleness (`cached` still answers the old numbers after prominence
30 → 90, then the refresh moves 0.8 → 1.0). The alternative — a silent recompute — would have looked correct and turned a
read into a write on every request.

**Operator-only, because the vector is a function of the operator's taste profile.** Publishing a work's vector would let
anybody with enough works invert the profile one coordinate at a time. A reader with no standing gets `404`, not `403`
(ADR 0030).

**The schema owned a word the header guessed wrong.** The probe header implies `shown/answered/dismissed`; migration
0054's CHECK says `positive | negative | neutral`, and `UNIQUE (account_id, work_id)` makes one answer per pair. The door
copies the migration's vocabulary, so a bad word is a `422` naming `engagement` rather than the CHECK surfacing as a `500`
— which is exactly what the mutant produced when the check was removed.

The ratchet moved six, not five: `taxonomy::tag_weights_for_work` was an orphan too and became reachable through the
vector producer. That is the transitive count doing its job — a door that calls a function that calls a query makes the
query reachable, and the number should say so.

Ratchet 131 -> 125.

### A promise the binary never asked it to keep (identity.rs, 2026-11-08)

`grant_guardian_authorization` carries the finding in its own header: before it, "a minor could never participate, whatever
the operator set up", because `guardian_authorizations` existed and nothing wrote to it. The function was written, the
documented reason was written, and no call site existed — so the instance advertised a guardian workflow it could not
perform. Four doors now, and each keeps one rule:

* **An age decision takes a reason.** It is the change with the least audit trail and the most consequence: a restricted
  account cannot participate, and the modlog's contract says every entry carries a reason. The `reason` field is
  `Option<String>` *in the type* so that its absence is this handler's 422 naming `reason`, not the JSON extractor's
  generic "Unprocessable Entity." — the field matters, so the error says which.
* **The guardian grant does both halves.** It records the authorisation *and* moves the state; the door reads the row back
  rather than assuming the second half, because a recorded authorisation that leaves the account stuck is a row nobody
  reads.
* **A fold points at a live account, and a tombstone is terminal.** Self-fold, a nonexistent target, a folded target and
  an already-folded source are all refused. The last one was found by the test, not by reading: the first version happily
  rewrote a tombstone's `folded_into`, which would silently re-attribute every row the first fold re-pointed.
* **Facts are operator-only, and `404` for a missing account.** The store returns `Option` precisely so the caller can
  tell "no such account" from "an account with nothing yet"; collapsing that into an empty object would have thrown the
  distinction away at the last step.

`count_excluding_system_accounts` got **no door**. Its own header said both of its callers were tests; inventing an
operator metric to lower a counter is bad faith, so it moved to `crates/test-support` and the store stopped exporting it.
Deleting a helper whose only callers are tests is the other legitimate end of this ratchet, and the one that needs no
surface invented for it.

**A live leak in the same module.** `GET /accounts/{id}/trust` took `RequireSession(_user)` — the underscore says nobody
looked — and answered any signed-in reader with another account's trust history: the modlog's record of one person, every
level change, why, and who made it. It is the account's own or an operator's now, `404` otherwise (ADR 0030).

Ratchet 125 -> 120.

### The exchange could only be switched on with SQL (exchange.rs, 2026-11-08)

`set_enabled` and `set_instance_id` had no caller. §11.17 makes the exchange a deliberately opt-in surface — "no request
shape turns on a server that did not enable it" — and the switch that turns it on was reachable only by an operator with a
database client. `list_latent_demand` had no caller either, so the instance accumulated demand it could not show anybody.

Three doors, merged into the existing `exchange` mount:

* **`GET|POST /admin/exchange/settings`** — the switch and the instance's own name in one place. `configured: false` is a
  different answer from `enabled: false`: a row that was never written is not a decision, and reporting the two the same
  way would make a fresh instance look like one an operator had deliberately closed. The first write creates the row with
  §11.17's 1000/hour default, so flipping the switch does not require knowing the second knob. The instance id is
  validated once (`[A-Za-z0-9._:-]`, ≤64 chars) because it travels as `source_instance` on every signal and is the dedup
  key for latent demand, and every accepted change writes a modlog row with a required reason.
* **`GET /admin/exchange/latent-demand`** — `signal_count` is how many times *this instance* has signalled, named that way
  and with no submitter or holder field anywhere, because §16.16.1 says re-signalling adds no weight and a reader count is
  not computable from the data the instance holds.
* **`GET /admin/exchange/works/{id}/provenance`** — `count_provenance_signals`, the function whose name exists to keep
  "provenance" from reading as "headcount". The response carries the number *and* a note saying it is not a count of
  readers or holders, and the door is `Operator` so the number does not appear on a public page where nobody could correct
  the misreading.

**The fifth orphan was a twin.** `exchange::list_review_queue` ran the same query as `taxonomy::list_unverified_nodes`,
with a narrower projection whose two extra columns (`curated_by`, `curated_at`) are always NULL — the query only selects
`review_status = 'unverified'`, and a curated row is not unverified. It was deleted, with the reason written where it
stood, and `GET /exchange/review-queue` keeps the taxonomy function. Deleting a redundant twin is the other end of this
ratchet, and the one that needs no surface invented for it.

**A guard mutant that survived for a reason worth keeping.** Removing the operator check from the latent-demand door left
the suite green: the test asserted the guard on two of the four doors and never asked the third. One reader assertion
later it reddens (200 vs 404). A guard mutant survives exactly the doors no test names, so the mutant is a map of the test's
blind spots as much as the code's.

Ratchet 120 -> 115.

### The mention that was never recorded (community.rs, 2026-11-08)

Five orphans, four of them missing surfaces and one a missing behaviour — and the behaviour was the interesting one.
`record_mentions` checked blocks and never checked mutes, and its own doc said *"Mute is checked separately in the
notification layer."* Nothing checked it anywhere. A reader who muted somebody still received mention notifications from
them: the one notification they had explicitly asked the instance not to send. `is_muted` gates the producer now, and the
comment says what it used to claim.

Making that true turned up three more defects, none of them in the code being added:

* **`post_comment` passed a pseud id where an account id belongs.** `mention_events.mentioned_by` references
  `accounts(id)`, so every comment mention failed its foreign key — and the insert was wrapped in `let _ =`, so the failure
  was silent in both directions: nothing recorded, nothing logged. Comment mentions had never worked. The same parameter is
  compared against each mentioned pseud's account to skip self-mentions, so that check could never fire either.
* **The mention parser refused hyphens.** `com-a` is a legal handle; the parser's character check was alphanumeric-or-
  underscore, so a hyphenated handle was permanently unmentionable and the symptom would read as "mentions don't work for
  that person". The trailing-punctuation trim keeps hyphens now.
* **`comment_by_id` selected eight columns and decoded eleven.** Its first caller — the permalink door — got a 500. The
  anchor columns are selected now, with the rule beside them: a `pub async fn` nobody calls cannot fail to decode, which
  is why this was invisible.

The four doors: `GET /comments/{id}` (anonymous; a deleted comment is the soft delete's tombstone, not a 404 — the comment
existed, and "no such comment" would rewrite history for everyone who replied), `GET /presence/{account}` (one person's
presence under the stream's **own** rule via `presence_visible_to`: muted or blocked is absent, and a person who turned
presence off is absent to others while their own read still answers `enabled: false` with no last-seen — an opt-out that
still reports when you were last here is not an opt-out), `member_role` in `GET /groups/{id}` as `viewer_role` (`null` is
"not a member", not "no access"), and `category_exists` at the **writer**: `create_topic_with_mode` refuses a category that
does not exist, which is what its own doc claimed and only one route enforced.

Ratchet 115 -> 110.

### A fresh instance had no categories (category_governance.rs, 2026-11-08)

This module was *partly* wired — `routes/directory.rs` imports it as `cg_db` and runs proposals, votes, vetoes, changelog
and the freeze — so the orphans were the edges of a working surface, and the edges were where the instance was broken in
ways nothing logged: `seed_categories` and `upsert_category` had no caller, so a fresh instance had **no categories at
all**. The directory's category filter offered nothing, and `create_proposal` answered "category not found" for every
§39.2 slug an operator would try. An empty list is a valid list; that is why it survived.

* **The seed runs at boot** through `crate::directory_seed::seed_directory_categories`, a named function so the count is
  what gets logged. It is idempotent *by construction* — `upsert_category` is `ON CONFLICT(slug) DO NOTHING`, which is
  §45.1's rule for a config upsert: it adds slugs the instance is missing and never resurrects a category the community
  removed. The test proves the second half by deprecating one and re-seeding. A failed seed logs and does not refuse the
  boot: an empty category list is a visible misconfiguration, a refused start is an outage.
* **The queue expires on read.** `expire_proposal` is a per-row sweep and the binary has no scheduler, so the read is the
  sweep — without it, `open` means "nobody has looked" rather than "still decidable". The route returns the number expired
  rather than implying it, and a second read finds nothing to do because the status already moved.
* **`count_active_categories` is the ceiling, visible and enforced.** `governance_state` reports `active_categories` and
  `at_ceiling`, and a create proposal is refused at **propose** time when the ceiling is reached: a proposal that cannot
  succeed spends quorum attention, and the queue is more expensive than the refusal.

**And a door that lied.** `POST /directory/categories/governance/max` ignored its body (`let _ = body.max;`) and answered
`200` with the unchanged configured value — a client reading that 200 could only conclude the change applied. The ceiling
is an instance configuration, so the honest answer is a `422` naming `directory.governance.max_active_categories` and
saying it is changed there; sending the value it already has is still a `200` the client can trust.

The boot wiring is asserted against `server.rs` source, the way `route_inventory` checks the route table against the
routers: `serve` binds a listener and no test may boot one, so the source is the only place the call can be observed.

Ratchet 110 -> 105.

### A table written and never read (story_identity.rs, 2026-11-08)

The grouping table was **written by the claim path and read by nobody**. `members_of` and `member_counts` had no caller, so
a work could accumulate external locations that no page ever listed, and "this work has one copy" and "this work has four"
were indistinguishable from outside — the import path did the expensive part (deciding two URLs are the same story) and
then kept the conclusion to itself.

`GET /works/{id}/preservation` now answers `editions` and `copies`. The list is **local copy first**, then each external
location: the copy a reader can open, then the ones they cannot. A work nobody has crossposted answers an empty list and
`0/0/0` rather than a 404 — "not crossposted" is a state, not a missing page, and the identity is created by the first
claim. `local` is computed as `total - external` from the same row set, so the three numbers cannot contradict each other.

**`merge_table_is_empty` was deleted rather than wired.** Its only caller was a test assertion, and its own doc said so:
"Phase E will have to delete this function or invert it, which is the point." The test now counts
`identity_merge_proposals` and `identity_merge_history` directly and carries the reasoning: a helper that always answered
`true` would have made the claim unfalsifiable, and that claim is the one standing between "the merge tables exist" and
"the merge tables are used".

Ratchet 105 -> 102.

### A monitor that was really a button (note_links.rs, 2026-11-08)

A note link was checked **only when its author pressed "re-check" on the work that carried it**. `find_needing_check` — the
queue — had no caller, so a dead link on an abandoned work was never noticed, and the `dead` count on that work's page was
a number that could not change on its own. The page reported a monitor; the instance had a button.

* **`POST /admin/notes/links/sweep`** (Operator, trust 5, `limit` clamped to 1..50) drains the queue's head through the
  same `check_note_link` the author button uses — one fetch path, one SSRF guard. A refused URL is recorded dead by the
  check itself, so one missing link never fails the sweep; a storage error is counted as `failed` and logged.
* **The queue has no "due" predicate**, so a sweep re-checks never-checked links first and then the oldest, and the handler
  says so. There is no scheduler in this binary (§37 puts recurring work on the operator): this is a button, and the
  scheduled form is named as the follow-up rather than pretended.
* **`GET /works/{id}/note-links?include=superseded`** answers the monitor's question instead of the reader's — every row
  the checker still holds, with `superseded` computed from the live set rather than guessed from `revision_id`. The plain
  read is unchanged: a corrected note still reports only what the work says now.
* **`count_dead_for_work` was deleted.** The route counts the dead rows of the same list it returns; a second source for
  that number could disagree with the list shown beside it.

**The fixture was the first thing the mutants found.** The first draft had a single link, so after the correction the live
set was empty — and a `superseded` flag that was always `true` passed, because there was nothing to compare it against. The
test now uses two chapters, one corrected and one kept, so the flag is exercised on a mixed set. Two further mutant attempts
never compiled and were re-targeted, not counted.

Ratchet 102 -> 99.

### A queue that could be neither filled nor emptied (engagement.rs, 2026-11-08)

`taste_notification_queue` had no producer and no drainer. Three store functions, a passing store test, and no caller in the
application — the table could be neither filled nor emptied, and migration 0055 states the intent in one sentence: *"When a
work is published, aligned users are queued for notification. The worker drains this table."* Neither half existed.

Both halves now do:

* **`queue_aligned_readers`** runs from `publish_work` (not on a replay: the queue is keyed by `(work, account)` so it would
  be a no-op, and a retried publish should not pay for a scan). It gates on the module switch itself, because the producer
  lives inside a core route where a module mount cannot gate it. It queues readers inside the operator's taste band, never
  the author for their own work, and skips a reader already holding `PENDING_CAP` unsent rows — which is what
  `pending_taste_notification_count` existed for.
* **"Aligned" is the instance's own notion**: distance to the operator's taste centroid, not a per-work relevance score.
  darkhold has no accumulated reader-by-work evidence (§49.9 refuses to invent it), and a notification that arrives for a
  reason the reader cannot name is worse than none.
* **`POST /admin/taste-notifications/drain`** delivers through `notifications::notify`, so the reader's own settings decide
  whether a row means anything. `delivered` counts rows *written* and the response says so.
* **`GET /me/taste-notifications`** answers the reader's own queue depth — a count, not the rows: a queued row is a plan,
  and showing it would deliver the notification without marking it sent.

**Three defects, none in the code being added.**

1. **`drain_taste_notification_queue` delivered twice.** It marked rows `sent_at = now` and then selected `WHERE sent_at =
   now` — and `now_rfc3339()` has **second** resolution, so a second drain in the same second re-read the first drain's
   rows. A queue whose entire purpose is exactly-once delivery handed everything to the worker twice. Rewritten as a single
   `UPDATE … RETURNING`. The test's second drain (same second) is the regression test, and it is what caught this.
2. **`list_users_by_centroid_distance` was undecodable on PostgreSQL** — `accounts.id` is a UUID column and the tuple
   decodes a `String`. Invisible while nothing called it.
3. **…and it failed on any account with no distance.** `taste_centroid_distance::float8` with no `IS NOT NULL`, on a column
   whose default is NULL on PostgreSQL (SQLite's is `0.0`): one unranked account and the producer queued nobody, silently.
   A reader with no distance is unranked, not unimaginably far.

**The first version of the test passed a mutant.** On the off instance the reader was also the publisher — and the author is
never queued for their own work, so author exclusion masked the module gate and removing the gate changed nothing. The
fixture now gives that instance a bystander reader and a separate author. That is the same lesson as the single-link note
test: a fixture whose two rules agree with each other cannot tell them apart.

Module row `taste_notifications`: `default_on: false` (it writes to people who did not ask), `Beta`, operator toggle, spec
cited as migration 0055 rather than the store's own `§9.9`, which points at Legal posture.

Ratchet 99 -> 95.

### Demand nobody recorded, and a guard that broke PostgreSQL (moderation.rs, 2026-11-08)

Three §35.5 signals had no producer and no reader.

* **A zero-result search is demand, not an error.** The forum search route now records a free-text query that finds nothing,
  keyed by the sha256 of the lowercased text so "Ten Ways" and "ten ways" are one miss with a count. A narrowing-only search
  (`category:` with no `q`) records nothing: that is a taxonomy question. `GET /supply/unmet-searches` (operator) is the read
  — a drought is a topic nobody writes about, an unmet search is a reader who went looking and came back empty.
* **`record_topic_activity`** runs on every reply, one row per `(topic, date)`: "last post at" cannot express forty replies
  today versus forty across forty days.
* **The first vote is the one an author remembers**, and it is notified once. Three gates, and only the last one is the
  enforcement: `existing.is_none()` (a *change* is not a new vote — the count alone cannot tell them apart), the count being
  exactly one, and the stored row. The store now **returns whether it inserted** (`rows_affected == 1` on both engines) and
  the caller notifies only on a genuine insert: a count that returns to one through a different reader after a retraction
  would otherwise send a second notice, and two casters racing on the same first vote would both send one.

**The defect this pass found was mine.** `category_exists`, the guard added in the community pass, used `SELECT 1 …` decoded
into an `i64` — `1` is **INT4** on PostgreSQL and sqlx will not decode it as INT8, so **every topic creation was a 500 on
PostgreSQL** while SQLite's untyped integers decoded happily. Reproduced with the change stashed (clean master fails the same
way) so it is not mistaken for this pass's edit, then fixed as `SELECT EXISTS(...)` decoded as `bool`.

The lesson is in the shape of the miss: that pass ran its own targeted test on both engines and the file it touched — but the
guard it added runs when a *topic* is created, and no test in that pass created one. A guard is only tested by the suites that
exercise the rows it guards, on both engines.

Ratchet 95 -> 92.

### A lease nobody renewed (jobs.rs, 2026-11-08)

The queue had producers and a worker. What it did not have was a caller for three of its own functions, and the first was
the one that mattered.

**`heartbeat`.** A claim is a *lease* — `options.lease`, 120 seconds by default — and `maintenance_pass` returns expired
leases to the queue. Nothing renewed it, so a job that outlived its claim was requeued by the sweep and handed to a
**second worker while the first was still writing it**. The same import, twice, over the same rows; the only trace was
"the lease was lost before the job could complete" in a log nobody reads on a success path. `run_once` now spawns a keeper
that renews at a third of the lease for exactly as long as the handler runs, and stops when the handler returns or when
the renewal says the lease is somebody else's.

Two facts about the test are worth keeping, because the first version of it proved nothing:

* **Only a queued job is claimable.** A second worker calling `claim_next` alone can never take a leased job; the sweep is
  what turns `leased` into `queued`. A thief that skips the sweep is a test of nothing — and it passed with renewal
  disabled, which is how it was caught.
* **The maintenance pass enqueues work of its own** (the syndication job). "The queue is empty afterwards" was the wrong
  assertion; the second worker legitimately leased that job. The assertion is scoped to the probe's id.

And a limit: lease stamps are written at **second resolution**, so a sub-second lease is expired the moment it is written
and no renewal interval can save it. The test uses a two-second lease against nearly five seconds of work.

**`counts_by_state`** is now `GET /admin/jobs/summary`, built from a new `ALL_STATES` in the domain rather than from
`GROUP BY state`: a dashboard assembled from the rows can only show states something happens to be in, which makes "no
failures today" and "nobody asked" the same line item. Every state is listed, empty or not.

**`attempts_for`** is `GET /jobs/{id}/attempts`. The owner sees their job's history — four attempts and the error from
each, not just the last one — and the operator additionally sees which worker ran it. A stranger gets the `404` an absent
job gets: a job id is not a capability.

Ratchet 92 -> 89.

### Three pieces of a cap, and none of them in the path (media_resilience.rs, 2026-11-08)

Four functions in this module had no caller, and the first was the whole anti-gaming rule for curator credits.

* **`sum_curator_rewards_today`.** The config declared `media_resilience.daily_credits_cap` (500), `caps::within_daily_cap`
  was a tested predicate, and this was the query that answers it — and the award path read **none of the three**. It
  hard-coded `15` and paid on every call, so the same mirror added a hundred times paid a hundred times. The link is still
  kept once the cap is reached (the correction is what the feature is for); the credits are paid up to the cap, partially
  when the remainder is smaller than the bounty, and the response says `credits_awarded` and `credits_capped` so a curator
  who is not being paid can see that instead of guessing.
* **`find_references_below_threshold`** is the under-mirrored queue the bounties are paid against. Without a caller, a
  curator could only find such a reference by stumbling into it. Now `GET /admin/media/references/at-risk?threshold=`,
  defaulting to the spec's own three.
* **`find_match_proposal`** is what happened to one proposal, which is the only record of a **refused** merge: the list
  route shows what is pending, and a rejected proposal leaves no other trace. Now `GET /media/match-proposals/{id}`.
* **`find_work_media_references`** is deleted, not wired: a narrower twin of `list_work_media_references`, which the
  `GET /works/{id}/media` route already calls and which answers the same rows plus each link's health and best mirror.

Both new PostgreSQL arms had never executed, and both were wrong — two of the three defects this pass fixed were in code
nobody had run:

| symptom | cause | fix |
|---|---|---|
| 500 on the queue | `m.id` is UUID, three timestamps are TIMESTAMPTZ, `content_notes` is JSONB; the row type is all `String` | `m.id::text`, `to_char(... AT TIME ZONE 'UTC', ...)`, `content_notes::text` |
| 500 on the queue, after the casts | a `to_char(...)` column is named `to_char`; `FromRow` maps by name | alias every expression (`AS first_seen_at`) |
| 500 on the proposal detail | `resolved_by` is UUID | `resolved_by::text` |
| `function pg_catalog.timezone(unknown, text) does not exist` | I copied the `to_char` treatment from `media_references` onto `media_match_proposals`, whose timestamps are **TEXT** | read the column, not the neighbour: plain `created_at` |

The last one is the sharpest: two tables in one file, four hundred lines apart, disagree about the type of `created_at`.

Ratchet 89 -> 85.

### A class nobody could read, from a table nothing filled (canon_agnostic.rs, 2026-11-08)

Three functions and a producer, and between them the whole of §50.2's class was invisible.

`stored_canon` had no reader, so a work's canon class — already computed, already stored, already tested — never reached a
page. `is_canon_agnostic` and `is_canon_dependent` are §50.2's eligibility question and its mirror; the module's own docs
insist a caller must not pattern-match `CanonVerdict` for this, because eligibility is a rule with one implementation.

And `measure_work_and_store` had no caller outside its tests, so on a fresh instance the table stayed empty, the listing
stayed empty, and "no canon-agnostic works" read as a fact about the corpus rather than a missing scheduler. It now runs
when a work is published — the moment its text stops changing — best-effort, like the taste queue beside it in `publish`.

What a reader gets:

* `GET /works/{id}/canon` — four states, not two: `canon_agnostic`, `canon_dependent`, `unclassified` (with the reason),
  `unmeasured`. A boolean would collapse three of them, and "we did not look" must not read as "we looked and it was fine".
  `eligible` and `assumes_canon` are separate because a badge and a filter are separate client decisions, and both are
  `false` when nothing was measured — the conservative direction §50.2 asks for.
* `GET /discover/canon-blind` — the labelled, bounded shelf: twelve rows, newest class first, every row carrying
  "you may not know this fandom", and **nothing ranked**, because a shelf with a score is a feed and §50.2 forbids these
  works outranking a recognised match. `state` distinguishes `unmeasured` from `ok`, so an empty shelf is not mistaken for
  an empty corpus.

**The missing half, named rather than approximated:** §50.2's rule is that a work is eligible when its fandom is unknown *to
the reader*. This pass ships the half that is a property of the work (is it readable without its canon) and the shelf; the
reader half needs each work's fandom tags intersected with that reader's taste weights, and it belongs in the taste module
rather than here — pooling per-reader weights into a per-work listing is the cross-reader blending §49.9 refuses. Until it
lands, the shelf is honest about what it is: canon-agnostic works, labelled, not "works you have never weighed".

Ratchet 85 -> 82.

