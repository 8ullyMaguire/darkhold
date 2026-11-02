# Handoff — the backlog's tail, measured (2026-11-02)

Where the tree stands, what is left, and the three things a cold start must know before touching
anything. Written after a session that closed M45-38b, M65-08, M45-36 (partly) and M45-62, and opened
M45-65.

## The Suki intake — 26 rows, `M69` (2026-11-02)

Suki is a booru: images, tags, ratings. darkhold is fiction: works, units, tropes, warnings. The intake
read one spec against the other and kept only what a fiction platform needs, in this project's
vocabulary. **Twenty-six rows, `M69-01..26`, all `planned`** — and one of them (`M69-25`, OpenSearch)
shipped the same day because it is three lines and always loses the argument for a slot.

The five worth reading before the other twenty-one:

- **M69-24** — one shared taste crate and JSON schema set, so suki and darkhold cannot drift on the
  blend, affinity or the passport. This is the row that keeps the rest honest.
- **M69-01..03** — instance DNA, auto-peering and a signed taste passport, in one wire format both
  products read, so a booru and a fiction archive can peer with each other.
- **M69-04** — tombstones that survive re-import. DMCA intake is planned (M45-52) and nothing today
  stops the same work returning through the importer, which is the failure a takedown exists to stop.
- **M69-10/11** — Glicko-2 with deviation and a calibration window; the arena is Elo at K=32, where two
  votes and two hundred share a ladder.
- **M69-16** — three-layer filtering; today every filter is equally overridable, including the
  illegal-content one that must not be.

Nothing needed to change for "everything is an extension": a planned row has no code and cannot default
on, and `OPT_IN` (modules/mod.rs) already fails a built surface that gets it wrong.

## The two checkouts are one lineage now (2026-11-02, later the same day)

`~/code/rust/darkhold` (the thinkcentre checkout, upstream `lorehaven`) and `~/code-local/rust/darkhold`
(this tree, upstream `darkhold`) shared an ancestor at `4b731acc` and had diverged. They are merged:
both checkouts stand at `5743f412`, this tree's history contains the other's three sign-in commits, and
the merge is pushed to `origin` (git.polarisocial.xyz) and `github` (8ullyMaguire/darkhold). Tags
`pre-merge-20261102` and `pre-merge-code-20261102` mark the tips in case the merge has to be undone.

Two things to know before trusting it:

- **The other checkout's own remotes were not pushed to.** `~/code/rust/darkhold` points at
  `github/8ullyMaguire/lorehaven` and `origin/git.polarisocial.xyz/.../lorehaven`; those are still on
  their old histories (1046 behind, 50 diverged). Publishing the merged lineage under the `lorehaven`
  name is a decision about which repository is canonical, not a `git push`.
- **The sign-in code came without tests.** `crates/app/src/routes/oidc.rs` (345 lines) and
  `crates/db/src/oidc.rs` (229) compile, their migrations (0147/0148) apply, and their columns now carry
  §11.16 decisions — but nobody has signed in through Polaris from this tree, and that needs a real
  estate account.

The four defects the merge surfaced are in `docs/verification.md` under the same date: a migration-number
collision, two undecided identity columns, one unclassified erasure column, and two design tokens that
only exist in this tree.

## The count, from the ledger rather than from feeling

```
docs/requirements.csv  883 rows
  implemented-fully-tested     409
  implemented-locally-tested   247
  implemented-verified-e2e     129
  planned                       93   (68 before the Suki intake, +26 rows, -1 shipped)
  unsupported                    4   M2-06, M6-10, M7-03, M6-15
  evaluated-and-rejected         1   M62-01
```

785 of 883 are implemented, at one of three strengths, and **the strength is the honest part of the
ledger**: `locally-tested` means the unit and integration tests ran on this machine; `fully-tested` adds
the second engine or the gate that would catch it drifting; `verified-e2e` means a browser or a live
instance answered. Every `planned` row names what it needs, and `--` in `evidence` means nothing ran.

`python3 scripts/ideas_catalogue.py` reports the same tree as 95 idea rows plus 74 module rows; the
idea catalogue is the *prospective* half and must not list anything the requirements say is built
(M65-08 — the assertion for that is `crates/domain/tests/ideas_match_requirements.rs` and it has now
caught two real stale rows, one of them mine).

## What this session changed, with its evidence

| commit | what | evidence |
|---|---|---|
| `c3a39a84` | the work page reports its notes' links: status per link, a warning before the click, author-only inline re-check, absent when a note holds no links | `e2e/note-links.spec.ts` **2/2 chromium against the real binary** — types the note into the editor, saves, publishes, presses the button |
| `2934387d` | the idea catalogue retires a built row through the id join, not just the title | `ideas_match_requirements` 3/3; the test was **red against the tree** for one commit after I closed M45-38 |
| `ae4809eb` | a media reference with no healthy link names what is gone (kind, author's note, host) instead of saying "unavailable" | WorkPage 12/12, routes 324/324, 3 mutations red |
| `da017f58` | §11.16's snapshot gate was **red in a clean tree since M47-14 and was not in CI**; decision written, gate wired; `events.created_by` / `event_participation.account` documented as pseud-holding; the 68-column class measured and opened as M45-65 | gate OK (2,414 columns / 329 tables), self-test exit 0, erasure invariants 4/4 |

Detail for each is in `docs/verification.md` under the same date. The two mistakes worth carrying
forward are there too: `getByText` fails on a sentence split by markup (the assertion was testing node
shape), and a leaked `isAuthorWork` mock made a feature look missing rather than the mock look wrong.

## The 68 that remain, by area

- **power-user, 20** — `M63-03, M63-05..M63-15, M63-18, M63-22..M63-26, M63-29, M63-30`. This is the
  `dh` CLI's contract work: `--sort` over any registered metric, `--jsonl/--table/--link` output
  shapes, saved queries with digests, a real query grammar with `--explain`, `lib sync/tag/diff/push`,
  `watch --exec`, `cron install` (systemd user timer), per-query feeds, `doctor`, profiles,
  `bench`, `audit`, `stats`, `tui`, Calibre as a source. Almost all are *CLI-shaped* and cheap to test
  against a scratch instance; several share one argument-parsing surface, so doing them in one pass
  beats twenty passes.
- **discovery, 14** — `M45-32` (trend radar) and `M64-01..M64-14` minus `M64-05`. M64 is the
  recommender-laboratory milestone: canonical surface ids, append-only impressions, daily arm
  snapshots, sequential promotion, the strategies (tag jaccard/co-occurrence, crosspost union, fandom
  graph, co-shelving, co-read, engagement overlap with k-anonymity, embeddings), taste gravity, facets.
  `M64-02/03` are the spine — nothing else in M64 is measurable without the impression log.
- **governance, 10** — `M45-52` (DSA notice-and-action), `M45-54` (RPF/Spain policy knob), `M45-55`
  (compliance tooling), `GS-1..GS-5` (guest access, guest fold, guest pseudonym, elastic trust from
  karma, jury from the eligible pool), `M29-08`/`M44-02` (feature-consensus board, Elo-ranked community
  board). GS-* is a chain: GS-1 gates the rest.
- **admin, 6** — `M45-45, M45-46, M45-47, M45-48, M45-50, M64-15`: versioned blendable taste, preview
  before apply, scale-aware activation (a mechanism declares a minimum activity and is dormant below
  it), hardware-aware presets, scheduled gravity, the experiment console.
- **catalogue, 4** — `M63-32` (fandom tags cover the catalogue), `M63-34` (summary search; **measure
  the LIKE over 141k rows before shipping it**), `M63-37` (per-source engagement as the source reports
  it), `M63-38` (quality weights per operator and per surface, not compiled in).
- **community, 3** — `M45-33` (beta/specialist matchmaking), `M45-34` (Fic Finder bounties),
  `M45-40` (curator lenses).
- **preservation, 3** — `M45-39` (instance-death mutual backup, dead-man's switch, redirect registry),
  `M45-58` (erasure reaches the supply routing record), `M66-08` (P2P local file sharing).
- **growth, 3** — `M45-41` (rec-post attribution), `M45-42` (claim your works), `M45-43` (Discord bot).
- **architecture, 2** — `M45-65` (below) and `M65-09` (deferred **by design**, see below).
- **one each** — `M45-37` (stable media identifiers: MusicBrainz/ISRC, Wikidata, ISBN/TMDB),
  `M45-44` (sister instances over ActivityPub), `M63-33` (reconcile the live database with the
  workspace migrations: 11 tables the migrations create).

## Read this before you pick up M45-65

The snapshot gate's `IDENTITY_NAME_RE` matches `account_id` and not a bare `account`; the erasure scan
matches `account_id`, `*_account`, `*_account_id` and not a bare `account` or `created_by`. Measured
from the migrations: **68** identity-carrying columns outside the foreign-key graph, **37** of them
invisible to both gates (25 bare `account`, 12 `created_by`).

**Do not start by widening the regex.** Both gates compare measured against classified in *both*
directions, so a wider pattern turns them red until all 68 columns carry a treatment — and each
treatment is a judgement (pseud vs account vs public handle) that has to be read off the writer:
`RequirePseud` or `RequireSession` in the handler that inserts the row. Widening the pattern and
leaving 68 entries blank produces a red gate where red is already background noise, which is exactly
what this session had to fix.

## Read this before you pick up M65-09

The per-user module lookup is **deferred on purpose**, not forgotten: the skip flag must be
initialised from the table at startup, bumped on every PATCH, cleared when the last row is deleted,
and would be wrong for a second process sharing the database. The row says to do it when a per_user
surface is hot enough to appear in a profile. Do not "optimise" it because it looks cheap.

## The one thing that needs a person

**`arena`'s `default_on`.** It is the only `Toggle::PerUser` surface that ranks people, and the
registry's own invariant (`OPT_IN` in `crates/domain/src/modules/mod.rs`) holds it off by design.
Turning it on is a product judgement about a surface that ranks people — not something to flip because
"everything relevant should be on".

## Re-run the gates before trusting anything here

```bash
export PATH="$HOME/.cargo/bin:$PATH"
python3 scripts/check-snapshot-pii.py                 # OK: 2414 columns across 329 tables
python3 scripts/check-snapshot-pii.py --self-test     # exit 0
python3 scripts/backlog.py                            # regenerates backlog.json + INVENTORY.md
python3 scripts/ideas_catalogue.py                    # 95 ideas; idempotent on a second run
cargo test -p darkhold-domain --test ideas_match_requirements
cargo test -p darkhold-db --test erasure_covers_fk_free_accounts

cd frontend
node node_modules/vitest/vitest.mjs run src/routes    # 324 tests, 46 files
node node_modules/vite/bin/vite.js build
# the binary embeds frontend/dist at COMPILE time — vite build alone serves nothing new:
cd .. && cargo build -p darkhold-app --bin darkhold
cd frontend && DARKHOLD_BIN=$PWD/../target/debug/darkhold \
  npx playwright test e2e/note-links.spec.ts          # serves a scratch instance on :8173
```

## If you resume cold, in this order

1. **`M63-02`-shaped CLI work first**: `M63-05` and `M63-07` (output shapes, real grammar). They are
   self-contained, testable against a scratch instance, and unblock most of the other 18 power-user
   rows by giving them a shared argument and rendering surface.
2. **`M64-02` + `M64-03`** (impression log, daily snapshots) — the spine the other eleven M64 rows need
   before anything in that milestone can be measured.
3. **`M45-65`** as an inventory pass with the writer-read method above, not as a regex change.
4. Everything else is open field; the areas above are grouped so one pass can carry several rows.
