# Darkhold

A self-hosted home for fanfiction: read without an account, write without a
publishing queue, import the library you already have, and take it offline when
you want it.

**North star:** the goal is not a feature count — it is to maximise the amount of *quality fanfiction
the operator would actually enjoy reading*. Every feature below serves one of three jobs: more of those
stories being written, the right ones finding the right reader, or the trust machinery that keeps both
happening. Anything that serves none of the three is scope, not progress.

*(Ascend has its own, narrower target: quality progression-fantasy fiction **with a system**. The
distinction matters — this platform is judged on everything the operator would enjoy, Ascend only on the
slice its builds can produce. A story that is excellent but has no system belongs here and not there.)*

Its sibling is **Ascend** (the build site for system fiction). How a work and a build point at each
other is written in `docs/NETWORK.md`: a seed out of Ascend, a URL back, no shared database and no
cross-service calls.

Darkhold is built from a written plan rather than by accretion. The plan, the
architecture decisions, and an honest log of what has actually been *executed*
all live in this repository.

## Status

**`docs/goal.md` is the standing brief** — build to completion, no stubs, and
the gates every change passes before it is committed.

This repository is the base for the consolidated from-scratch specification. The
honest one-line status, re-derived from `docs/requirements.csv` (895 rows):
**812 implemented** (429 fully-tested, 129 verified E2E, 254 built but not
verified end to end), **78 planned**, **4 deliberately unsupported**, **1
evaluated and rejected**, tags through `v0.51.0`. The planned remainder is
concentrated in a few blocks — M69 (25) · M63 (17) · M64 (14) · M45 (13) · GS (5) — and `docs/plans/remaining-work.md` tracks them; the M45 block is the
taste and catalogue block the last sessions have been closing a row at a time.
`docs/plans/bots-extension.md` covers the bot core, the separate
Lorebook desktop app, and a Chromium extension that does not exist yet.

**The metadata exchange** (spec §11.17, §15.17, §19.14, §2.3.1) was adopted
2026-09-25 after triaging an external proposal. Readers may offer metadata
signals about works they hold and receive community-curated canonical metadata
back. Its first rule is the one that matters: **a signal is a fact about a work,
never a fact about a reader** — reading history, ratings, notes, private library
membership and credentials are not fields the signal type has, so no
configuration can widen what leaves the instance. It is opt-in per instance and
independent of the analytics tier. The proposal as submitted could not be applied
— roughly a third of its section citations were wrong — so the reasoning,
corrections and rejections are in `docs/plans/metadata-exchange-triage.md`.

The build is organized as worked milestones that do not correspond one-to-one
to spec §-numbers — `docs/plans/junior-implementation-plan.md` §0 maps them.
`docs/requirements.csv` lists every requirement with a status (the row-by-row
authority, and the seed for the roadmap board), and `docs/verification.md`
records the evidence behind each claim.

| Milestone | Area | State |
|---|---|---|
| M0–M15 | Platform core: tooling, design system, accounts/pseuds, publishing, reader, jobs, imports, exports, library, taxonomy/search, discovery, positivity, community, governance, economy | Built and locally tested (see requirements.csv for per-row status) |
| M16–M26 | Marketplace, translation, public API surface, admin/ops, monetization, media skeleton, comments/CSV, archive mode, TTS | Built (contract + bodies; per-row status in requirements.csv) |
| M31–M35 | Forum series: categories, read state, thread modes, spoilers, forks, sparklines | Built and locally tested |
| M39–M47 | Directory, fork provenance, half-life, export CTAs, browse ordering, roadmap consensus (Elo board), work aggregates, user settings | Built (M45/M47 have planned residual rows) |
| M48–M51 | Media resilience: import rescue, health dashboards, mirrors, reverse search | Built — tag `v0.51.0` |
| M52–M55 | Rec strategy registry, adapter porting, companion bot, OpenAPI publication | Planned — `docs/plans/remaining-work.md` |
| M57 | Metadata exchange: signals out, canonical metadata in, auto-created taxonomy entities | Specified (spec §11.17, §15.17, §19.14) — not built |

No screen in this repository displays mock data. Pages that exist show real
values from the server; routes that are linked but unbuilt say so plainly.

## Requirements

- Rust 1.82 or newer (developed on 1.98)
- Node 20 or newer (developed on 26) — only to build the frontend
- SQLite (default) or PostgreSQL

## Quick start

```bash
cargo build                       # builds without the frontend (uses a placeholder page)
cd frontend && npm ci && npm run build && cd ..
cargo build                       # now embeds the real bundle

cargo run -- migrate              # apply migrations
cargo run -- seed --development   # a development account, pseuds and defaults
cargo run -- doctor               # is this instance healthy?
cargo run -- serve                # http://127.0.0.1:8080
```

Then look at:

```
/health/live      process liveness
/health/ready     database, schema and storage checks
/api/v1/meta      instance metadata, including the active content policy
```

## Command line

```
darkhold serve               start the HTTP server
darkhold migrate [--status]  apply or inspect migrations
darkhold seed --development  write development fixtures (refuses production)
darkhold doctor [--strict]   check configuration, database, storage, converters
```

Configuration precedence is **argument → environment → file → default**. Copy
`darkhold.toml.example` to `darkhold.toml`, or use `DARKHOLD_*` environment
variables (see `.env.example`).

Production is strict by design: `serve` refuses to start if cookies would not be
`Secure`, if CSRF protection is off, if the development seeder is enabled, if the
public URL is not HTTPS, or if storage is still the relative development path.
Each refusal names the setting to change.

## Repository layout

```
crates/
  domain/     identifiers, error taxonomy, resource policies — no I/O, no transport
  db/         SQLite and PostgreSQL access, migrations, repositories
  app/        CLI, configuration, HTTP surface, embedded frontend, worker entry point
frontend/     Svelte 5 + TypeScript, built to static assets and embedded in the binary
migrations/
  sqlite/     one set per dialect, kept identical by a test
  postgres/
docs/
  goal.md               the standing goal: build to completion, no stubs, and the gates
  adr/                  architecture decision records
  design/THEME.md       "The Reading Room" — the visual specification
  spec.md               the 18-milestone implementation plan every decision is measured against
  spec-gaps-ficnexus.md features the older FicNexus build has that this plan does not; review draft, not adopted
  tutorial/             build it yourself, milestone by milestone
  requirements.csv      every requirement and its status
  verification.md       what has actually been run, and what has not
  plans/
    remaining-work.md           the forward plan: what is left, in what order
    bots-extension.md           the bot core, the local desktop app, and the Chromium extension
    metadata-exchange-triage.md an external proposal reviewed, corrected and partly adopted
    junior-implementation-plan.md  the historical milestone map
```

## Design

The interface is **"The Reading Room"**: warm paper, deep evergreen, muted
copper, bookish headings over clean interface text, and almost no decoration in
the reader. `docs/design/THEME.md` is the specification;
`frontend/src/styles/tokens.css` is its implementation. Three presets ship —
Reading Room, After Hours, Clear Day — and reader appearance is deliberately
independent of the site theme.

## Contributing to your own instance

This is meant to be self-hosted software. Every limit and toggle is a
configuration value with a documented default rather than a number in the code,
and `darkhold doctor` reports the state of the machine it is running on.

## Running the gates

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets > /tmp/clippy.log 2>&1   # stderr matters
cargo test  --workspace --no-fail-fast  > /tmp/test.log   2>&1
```

Redirect with `2>&1` and read the file. `rustc`, `clippy` and `cargo test`
report warnings and errors on **stderr**, so a check piped through
`grep -c warning` with `2>/dev/null` in front of it returns a confident zero
from an empty stdout. That mistake sat in this repo's gate for several
sessions, reporting "0 warnings" while the tree held 30 — three of them real
defects, including a test that had never run. If a gate has never once failed,
suspect the gate before you trust it. Background: `docs/verification.md`.

A green E2E deserves the same suspicion. `frontend/e2e/coverage.spec.ts` asserted
that the export list was empty after deleting one export, on an account shared
with the test above it — so it could never pass, and the failure read as a
product bug. Assert the specific thing you changed, using a stable per-row
handle, and count rows before and after.

## The terminal, `dh`

`crates/cli` is a query tool over `/api/v1`, not a GUI over the site's screens:

```
dh rec --fandom naruto --updated 7d --sort likes --limit 10
dh rec --fandom naruto --updated 7d --engine rrf --explain
```

The window grammar is the server's, so a window typed here cannot mean something
different from the same window in a URL: `7d`, `>30d`, `@2026-10-27`,
`2026-01-01..2026-10-15`. Naming an engine (`--engine rrf|legacy|<strategy>`)
needs `DARKHOLD_TOKEN`, because the engines rank against what the calling account
has already read; a plain catalogue query does not.

`--sort` takes two kinds of name and answers both, never `unknown sort` for a
name this product has an opinion about. `dh rec --list-metrics` prints the whole
vocabulary with each name's status:

```
dh rec --sort kudos                 # AO3's word for the imported count (= likes)
dh rec --sort completion            # complete, ongoing, hiatus, unknown, cancelled
dh rec --sort freshness_weighted_quality   # quality × exp(-days/180)
dh rec --sort wilson ; echo $?      # 2: "not available yet — views are null … (M63-32 imports them)"
```

A computed metric ranks in memory and answers with the gate (`ranked N of M`); a
stored column is an `ORDER BY`; and a name whose input does not exist yet is a
`422` (exit 2) that names both the missing input and the requirement row that
supplies it. A word nobody registered is still refused, so a typo cannot be
answered with a likes order under another name.

A query can be named and rerun, and a rerun can say what is new:

```
dh rec --fandom naruto --updated 7d --save weekly     # stores the query string itself
dh rec --saved weekly                                 # replays it, byte for byte
dh rec --saved weekly --digest-diff                   # only rows not on the last run
dh rec --queries                                      # name, kind, when it last changed
```

What is stored is the query string this call would have sent, so
`--saved weekly` asks the catalogue exactly what `--save` asked — a stored
document that re-encoded the parameters would drift precisely where a window is
anchored. `--digest-diff` diffs **ids against the previous answer**, not against a
timestamp: a row can enter a `--updated 7d` page because its author posted, because
a recheck moved `source_updated_at`, or because the window slid, and only the last
answer can say which ids are new. The first run reports everything and says so
(`first run — nothing to diff against`). Saving, listing and diffing need the **session cookie**
(`DARKHOLD_COOKIE`), not `DARKHOLD_TOKEN`: the saved-view routes are session routes, and a bearer
token gets a 401 there (measured). A view belongs to an account, so a view id is not a capability —
a run against somebody else's view is a 404, not an empty diff.

Those three verbs — list, read, record a run — also accept a **bearer token carrying
`library.read`**, because a scheduled run cannot hold a cookie that expires in days. The verbs that
create, rename and delete a view are still session-only: a token minted to read a view every six hours
has no business deleting it, and the server answers `401` (wrong kind of credential) rather than `403`
(too few scopes). `dh cron install` is what that widening is for:

```
dh cron install --saved weekly --every 6h --jitter 10m      # writes user units, prints the systemctl lines
dh cron install --saved weekly --every 6h --dry-run         # prints the units, writes nothing
dh cron list                                                # what is installed, read from the units
```

Three files under `~/.config/darkhold/cron/`, one per query: `<name>.timer`, `<name>.service`, and
`<name>.env` holding `DARKHOLD_TOKEN` at mode `0600` and never printed. The service runs
`dh rec --saved <name> --digest-diff`, so a timer's output is the difference since its last run. It
does not run `systemctl`; it prints `systemctl --user daemon-reload` and
`systemctl --user enable --now <name>.timer`, because a CLI that shells out to systemctl fails
differently on every distro. `--every` takes a number and one of `s`/`m`/`h`/`d`: a bare `6` is
refused, since systemd would read it as six seconds.

`dh rec` searches a title and sorts a catalogue page. `dh q` is the other half: one expression in
the full query language, sent to the same `/api/v1/search` endpoint the site's search box posts to,
so an expression cannot mean one thing in a terminal and another in a URL:

```
dh q 'tag:naruto AND kudos:>=100'
dh q '(tag:romance OR tag:angst) AND updated>=-7d' --jsonl --fields title,author,words
```

The grammar is the server's: `tag:`, `fandom:`, `AND`/`OR`/`NOT`, parentheses, `kudos:>=`,
`words:>`, `completion:`, `status:`, and `updated:`/`published:` as either a date
(`published:>=2026-08-01`) or an age (`updated>=-7d` — the same age spelling the catalogue's
`--updated` takes). A query the parser rejects is a `422` and exit code 2: the sentence names the
token, which works spelling to use instead, and a caret sits under the character the parser stopped
at. There is no second parser here to disagree with it.

```
   $ dh q 'kudos>=100'
   kudos>=100
   ^
   `kudos>=100` is free text: comparisons go after a colon, so write `kudos:>=100` at offset 0
   (exit 2)
```

The caret sits at the parser's offset, which for a mistake that is a whole token is the token's
first character.

An expression worth keeping goes in a file, because a file is versioned, diffable and reviewable in a
way a shell history is not. `--from-file` reads it from `.darkhold/queries/<name>.q` (a name with a
slash is a path; a file beside the caller is the fallback):

```
   .darkhold/queries/weekly.q
     # weekly scan: naruto, updated this week, at least 100 kudos
     tag:naruto AND updated>=-7d
     AND kudos:>=100

   $ dh q --from-file weekly
```

`#` starts a comment at the start of a line or after whitespace — a tag spelled `#angst` survives,
because there is no escape rule in the grammar to inherit. Blank lines are skipped, each line is
trimmed, and the lines are joined with one space, which is the string the server parses. The offset map
is kept, so a rejection points into the file and not into a join the reader never typed:

```
   $ dh q --from-file bad
   .darkhold/queries/bad.q:3:12
   kudos:>=abc
              ^
   kudos >= takes an integer, got "abc" at offset 26
   (exit 2)
```

Passing an expression as an argument *and* `--from-file` is refused: one of the two would be silently
ignored, and silence is the one thing a query tool cannot afford.

`dh q --explain` prints the **query plan** instead of hits — the engine that planned it, the
statement it was asked about, the access path, and the indexes the plan names:

```
   $ dh q 'tag:naruto AND words:>1000' --explain
   engine: sqlite

   statement:
     SELECT * FROM (SELECT works.id, works.title, … LIMIT ?

   plan:
     CO-ROUTINE s
     SEARCH works USING INDEX works_listing (lifecycle=? AND visibility=?)
     SEARCH r USING COVERING INDEX sqlite_autoindex_work_tags_1 (work_id=? AND node_id=?)
     USE TEMP B-TREE FOR ORDER BY

   indexes: works_listing, sqlite_autoindex_work_tags_1, units_work_order, …
```

That is the engine's own `EXPLAIN QUERY PLAN` (SQLite) or `EXPLAIN` (PostgreSQL), run against the
statement this search would execute, with this search's binds and limit. A plan for a statement
nobody runs answers a question nobody asked, and would go on answering it after the real statement
changed. `--explain` with `--jsonl`, `--cols` or `--fields` is refused: a plan has no columns to
project.

`dh rec --explain` is the other one: it prints each strategy's production and its match inside the
filtered set, which is how a strategy that contributes nothing becomes visible.

**The catalogue's fandom coverage is partial, and `dh rec --fandom` says so.**
On the live instance 7,626 of 141,317 rows carry a fandom tag (5.4 %, 233
fandoms), so "Naruto" answers 265 rows — those are the tagged ones, not every
Naruto fic in the catalogue. The coverage line is printed with every fandom
query for exactly that reason.

## Where this lives

| | |
|---|---|
| live | <https://darkhold.polarisocial.xyz> |
| forge (self-hosted) | <https://git.polarisocial.xyz/hirrolot19/darkhold> |
| mirror (GitHub) | <https://github.com/8ullyMaguire/darkhold> |

Both forges carry `master` and every tag. They are mirrors of one history, not two lines: push to
both, and check them against each other rather than trusting one.

```
git push origin master && git push github master && git push origin --tags && git push github --tags
```

The repository was called `lorehaven` until November 2026; both forges were renamed to `darkhold`,
and the old deployed line is kept on each forge as the branch `archive/lorehaven-deployed`. Its
topic lists differ by number — GitHub caps a repository at 20 topics and Forgejo at 25 — so the
self-hosted repo carries five the GitHub mirror cannot. That is a platform limit, not drift.

## Licence

AGPL-3.0-or-later.
