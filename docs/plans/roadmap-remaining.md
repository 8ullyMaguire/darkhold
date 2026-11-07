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
