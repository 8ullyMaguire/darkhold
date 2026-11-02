---
title: "Handoff — M45-30 live, one requirement row measured false, and the image-preservation line opened"
date: 2026-11-02
status: shipped-and-live
branch: master
commit: eeccc436
live: 0.1.0+ed9bc650
---

# Handoff — darkhold, 2026-11-02 10:09

Picks up where `docs/handoff.md` §M45-30 leaves off. Written after the milestone was deployed and
proven live, then after measuring the queue for what to do next. Nothing is uncommitted; nothing is
half-deployed.

## TL;DR

M45-30 (cross-language supply) is **live and verified by hand**. Three things were learned while
looking at what is next and they change the queue:

1. **M63-33 is a false alarm** — its "11 absent tables" are eight renames plus three orphan tables
   nothing reads. Do not spend a milestone on it.
2. **The image machinery is fully built and has never seen a byte** — 32 orphan references, zero
   work↔image links, zero health checks, and every import reports `total_urls: 0` while 1,375
   revisions carry `<img>`.
3. **The deploy recipe taught a stale binary** — `./target/release/darkhold migrate` silently did
   nothing; the unit's own binary is `~/.cargo-target/darkhold/release/darkhold`.

Next milestone, in order: fix the media-rescue counter (half a turn), then the image-preservation
vertical slice. Both are argued in `~/secondbrain/10-Projects/darkhold/2026-11-02-next-ten-suggestions.md`.

## 1 · State, verified

| thing | value |
| --- | --- |
| commits | `ed9bc650` (M45-30), `0582e272` (live record), `b643606a` (mutation-claim correction), `eeccc436` (OPT_IN fix) |
| remotes | `origin` and `github` both at `eeccc436`; thinkcentre checkout fast-forwarded |
| live | `0.1.0+ed9bc650`, environment `production`, 134 migrations |
| catalogue | 101 modules, 175 ideas |
| working tree | clean on all three checkouts (`/home/alvaro/code-local/rust/darkhold`, `code/rust/darkhold` mirror, thinkcentre `/personal/documents/code/rust/darkhold`) |
| tests at HEAD | domain **846** + db 159 + app 236 green; `m65_user_overrides` 7/7, `m65_module_host` 8/8, cli 16/16 |

## 2 · What M45-30 shipped

- `GET /translations/supply` — counts by primary subtag, a computed `unrepresented` list, and a
  `transfer` block that is **`available: false` with the missing inputs named** (`stylometry`,
  `tag backfill (M63-32)`). No plausible float.
- `GET /translations/bounties`, `POST /works/{id}/translation-bounty` — funding rides the existing
  `bounties` table with `job_kind = 'translation'`; `pt-BR` normalises to `pt`.
- **§40.2 made true.** The spec claimed "the translation pipeline (M17) checks `translation` before a
  request is created"; measured, `POST /works/{id}/translations` created the job without reading the
  statement. It now refuses on the field `translation`, with a sentence per statement.
- Two rules with tests: **permission before money** (a balance refusal would tell a patron to top up
  for something the author forbade), and **`unstated` refuses** (silence is not consent).
- No migration: the fourth milestone in a row to reuse a schema, because `translation.rs` in three
  crates and a registry row already existed (M17).

Deferred **by name**: claiming/settling a bounty (§20.3's), the `ask` permission-request workflow
(refused with that as the reason), language detection (`works.language` is declared by the importer).

## 3 · Verification, and one correction

10 mutations, 10 caught. Two first cuts were **compile errors**, which is not a verdict:

- **C** (balance checked before permission) was re-cut and then **survived** — the guard test gave
  the "poor" patron a fresh account, which holds starter credits, so both orders refused on the
  statement. The proof now lives in a store test with an account holding nothing and a route test
  whose amount (999,999,999) no balance can cover.
- **G** (§40.2 gate removed) shipped in this handoff as "both were re-cut" while
  `m4530_recut.json` held only C. G was re-cut for real afterwards — `let _ = permission;` keeps the
  binding read under `-D warnings` — and
  `creating_a_translation_is_refused_unless_the_author_said_yes` catches it (5 passed, 1 failed).
  The paragraph in `docs/handoff.md` now names that test instead of asserting the re-cut.

**Live proof** (hand-run, probe rows deleted afterwards — works back to 58,042, bounties 0,
translation_jobs 0): the report answers as designed; an `unstated` work's request is 422 naming
`unstated`; after `yes` the same request returns a job; a credited probe funded `pt-BR → pt` and the
public listing carried `terms {source_language: en, target_language: pt}`; flipping to `ask` **with
the credits still in hand** refused on `translation` — the statement, not the balance, is the gate.

## 4 · Divergences measured today

**M63-33 is false.** `migrations/postgres` declares 324 tables, live has 325. The 11 "absent" are
created then renamed — `0122_rename_chapters_to_units.sql` (`chapters→units`,
`chapter_revisions→revisions`, `import_chapters→import_units`) and `0124_canonical_entities.sql`
(`taxonomy_nodes→canonical_entities`, `taxonomy_aliases→entity_aliases`, `taxonomy_node_scope→
entity_scope`, `taxonomy_edges→entity_edges`, `taxonomy_closure→entity_closure`,
`generated_content_policy_new→generated_content_policy`). Three have no migration at all —
`federation_state`, `governance_freeze` (**0 Rust references** each) and `entry_moderation` (its 23
hits are the different table `entry_moderation_proposals`). Correct the row; do not write renames.

**The image line is inert.** `media_references` 32, `availability_links` 32 (all `pending_verification`,
all `content_hash = 'pending'`), `work_media_references` **0**, `link_health_checks` **0**,
`local_mirrors` **0**, `preservation_destinations` **0**. All 32 references were created on
2026-10-08. 88 import reports carry `media_rescue` and every one sampled reads
`{"total_urls": 0, "new_references": 0, "unparseable": 0}` — while **1,375 revisions contain
`<img>`** across 58,042 works. The counter cannot distinguish a constraint violation from a
connection error from a policy refusal, which is why nobody knows which of those is true.

**The archive is losing art it cannot even show.** CSP is `img-src 'self' data:` — no imported image
renders on darkhold at all. Hosts across those 1,375 revisions: `cdn.discordapp.com` 297 (signed
links, ~24h), googleusercontent 271, `i.pinimg.com` 67, `media.discordapp.net` 39,
`cdn.fiction.live` 40, `cdn.donmai.us` 24, wixmp 21, `i.postimg.cc` 20.

**M45-29 left a module invariant red, and no targeted test run can see it.** `finishers` shipped
with `default_on: false` and never entered `OPT_IN`, so at HEAD
`only_planned_and_opt_in_surfaces_default_off` and
`a_valid_table_resolves_and_the_default_is_every_surface_but_the_opt_ins` failed — the row is built,
off, and neither planned nor opt-in. The M45-29 session ran `cargo test … finishers` and its own
store tests, all of which pass; the invariant lives in the crate's **lib** tests and only runs when
the whole crate is tested. Fixed by adding the deliberate seventh entry to `OPT_IN` (`eeccc436`);
domain lib went 846 passed, 0 failed. Nothing about the deployed binary changes — `OPT_IN` is a
test-time constant — but this is the second time this session that a green targeted run hid a red
crate.

**`bounties.created_by` is written empty** by `create_bounty` (§20.3's own shape); the patron lives in
`account`, so a public listing shows a blank funder.

## 5 · Traps this run paid for

- **`./target/release/darkhold` is a stale binary** (Oct 29, predates `[modules]`). It rejects the
  config with `unknown field 'modules'` and, under a `| tail -3`, the failure is invisible, so
  `migrate` silently does nothing. Always `~/.cargo-target/darkhold/release/darkhold migrate`
  (verified: *"schema already up to date: 134 migration(s)"*). Still taught wrongly by
  `scripts/budget.sh` and three handoffs — see suggestion 9.
- `psql -c` with several statements **aborts the whole batch** on the first error; delete one table
  per `-c`.
- Column names that cost a round trip each: `translation_jobs.source_work` is **TEXT** (join
  `works.id::text`), `derivative_lineage.from_work_id` / `to_work_id` are UUID,
  `work_media_references.unit_id` is UUID.
- A fresh **live** account holds **0** credits, while the test harness grants starter credits. That
  asymmetry is why the order test needed an amount above any balance.
- The session cookie is `Secure`: a `CookieJar` refuses to store it for `http://127.0.0.1`. A live
  probe must carry the cookies by hand.

## 6 · The queue

Full argument in `~/secondbrain/10-Projects/darkhold/2026-11-02-next-ten-suggestions.md`. Order:

1. Media-rescue counter per reason, re-run one import *(half a turn — blocks everything below it)*
2. Retire M63-33 *(15 minutes)*
3. Image-preservation vertical slice *(one milestone)*
4. M45-61 + M45-62 — `comments.created_at` compared as text; pseud ids in account-named columns
5. M66-09 — the hit-rate HTTP door (`hit_rate.rs` exists, no route)
6. M63-32 — the tag backfill (7,626 of 141,317 rows tagged; unlocks M45-30's refusal)
7. M64-01/02 — surfaces with competing algorithms and an impression log *(needs its own plan)*
8. M45-39 — instance-death protection: mutual backup, dead-man's switch
9. Make a stale binary impossible to deploy (read `ExecStart`, refuse an old binary, print version)
10. M65-08 — idea→module without a second backlog

## 7 · The image line, in one paragraph

darkhold is the **citation ledger**; suki is the **byte vault**. darkhold knows which image is cited
by which story, under which permission; suki has md5-keyed dedupe, `dhash` near-duplicate merging, a
disk cache that fetches each file once and a **permanent layer whose pinned files are never evicted**,
plus `mirrors.rs` with `MIN_COPIES = 3` and "alive is a fact with an expiry". The flow: cite →
`POST /api/v1/preserve` with provenance → fetch once, md5+dhash → pin → serve `/media/preserved/{ref}`
from darkhold (302), which is the only way art renders under `img-src 'self'`. Consent: the work's
`redistribution` statement gates it, suki's `preserve` is the escape hatch, takedowns propagate, and
preserved art is **not** republished into suki's browse/search. Twenty ideas with default-on/off
values: `~/secondbrain/10-Projects/darkhold/2026-11-02-darkhold-suki-image-preservation.md`.

## 8 · Commands that work

```bash
# tests, local (SQLite first, then PostgreSQL through the container)
cd /home/alvaro/code-local/rust/darkhold
cargo test -p darkhold-domain --lib translation
cargo test -p darkhold-db --test translations          # add DARKHOLD_TEST_PG_URL for PG
cargo test -p darkhold-app --test m45_30_translations

# deploy (the binary the unit actually starts)
cd /personal/documents/code/rust/darkhold && git fetch -q origin && git merge --ff-only origin/master
CARGO_TARGET_DIR=~/.cargo-target/darkhold cargo build --release -p darkhold-app --bin darkhold
cp ~/.cargo-target/darkhold/release/darkhold ~/.cargo-target/darkhold/release/darkhold.prev
systemctl --user stop darkhold
~/.cargo-target/darkhold/release/darkhold migrate
git rev-parse --short HEAD > darkhold.built-from
systemctl --user start darkhold && systemctl --user is-active darkhold
curl -s http://127.0.0.1:8081/health/live
```

## 9 · Do not

- Re-pick **M63-33**. Its premise was measured false today; the row needs correcting, not building.
- Publish preserved art into suki's public surfaces by default. A private copy for a work's readers
  is preservation; a new public booru post is republication.
- Trust `./target/release/darkhold` for anything, including `migrate`.
- Treat `media_rescue`'s `unparseable` as a number until it is split per reason.
