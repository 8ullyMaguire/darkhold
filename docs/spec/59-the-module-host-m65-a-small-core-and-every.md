# 59. The module host (M65): a small core, and every surface as a switchable module

**Status:** reserved
**Milestone:** —
**Source:** §59 of `docs/spec.md`

**Measured before writing this (2026-11-01):** `crates/app/src/routes/` holds **82 modules**,
`server.rs` mounts **79** of them across **90 merge sites**, `config.rs` is **6,160 lines** with
**49 top-level sections**, and `/api/v1/extensions` **already exists** — `marketplace.rs` serves a
third-party extension catalogue with `GET /extensions`, `GET /extensions/{slug}`, grant/revoke and
`extension_submissions` behind it (migration 0113).

That collision is why this section uses a different noun. In this product an **extension** is an
installable third-party package (§6.3, Milestone 15: manifests, grants, reviews, webhooks). A
**module** is a compiled-in surface of the server itself. `AppError::ModuleUnavailable` and
`MODULE_UNAVAILABLE` already meant exactly that, and `works.rs` already answered
`ModuleUnavailable { module: "taste" }` when a surface was off — so §59 fills a vocabulary the
tree had and a mechanism it did not.

## 59.1 Compiled in, not loaded

A module is Rust code in the binary plus a row in the registry. There is no `dlopen`, no ABI, no
second process, no untrusted code. Third-party loading stays Milestone 15's problem, and it will
be a *producer* of these rows, not a parallel system.

The reason is not conservatism. A plugin ABI in a `tokio` server costs a boundary that leaks
panics, blocking calls and unwinding; and what an operator actually asked for — a small core, and
every surface individually switchable per instance and per user — needs **a registry and a
refusal**, not a loader.

## 59.2 The core is what cannot be switched off

The core is `meta`, `health`, `auth`, `settings`, `openapi`, the request plumbing (rate limiting,
static assets, CORS, request ids), the database layer, and the host itself, plus the module
catalogue (a page must be able to ask why it is empty). Everything else — including surfaces an
instance could not usefully run without, like `works` and `search` — is a module whose compiled
default is **on**. That asymmetry is deliberate: a surface that can be disabled is a surface whose
absence gets tested, and the core stays small enough to audit in one sitting.

Every row carries:

| field | meaning |
|---|---|
| `id` | lower snake case, stable, what config, the API and the CLI speak |
| `default_on` | whether a fresh instance runs it; **true for every module that exists today** |
| `stability` | `stable`, `beta`, `experimental`, `planned` — a `planned` row has no routes |
| `toggle` | `fixed` (never switchable: safety surfaces), `operator`, `per_user` |
| `requires` | ids that must also be on |
| `milestone`, `spec` | provenance, so a disabled surface explains itself |
| `nav` | the navigation entries it contributes (§6.3 slots) |

## 59.3 Resolution: instance, then user, and never widening

```text
compiled default  →  darkhold.toml [modules]  →  account override  →  resolved
```

- `[modules] disabled = ["quiz"]`, `enabled = [...]`, `allow_user_overrides = true` (default).
- An unknown id is a **startup error naming the nearest registered id**. A `fixed` id in either
  list is refused quoting §59.4. An id in both lists is refused, because a file that says two
  things about one module has no reader that can obey both. A `planned` id in `enabled` is refused
  naming its milestone. A `requires` that will not be enabled is refused naming both.
- A user preference can **narrow** an instance decision and never widen it: enabling what the
  instance disabled is not a state that exists, and the API reports `refused: "instance"` rather
  than storing a preference nobody honours.
- `allow_user_overrides = false` removes the user layer entirely and the write route refuses,
  naming the key. A key with no reader is a comment an operator believes is a control.

## 59.4 Safety surfaces are fixed, and the list is short on purpose

`moderation`, `erasure`, `privacy`, `account_permissions`, `admin`, `health`, `auth`, `settings`,
`meta`, `openapi`, `retention_settle`, `reader_body_copies`, `imports` and `jobs` are
`toggle: fixed`. They are not features; they are the promises an instance makes. Everything else
— including `search`, `economy`, `supply`, `draw` — is switchable, because an instance that cannot
turn off its own slot machine is not configurable, it is decorated.

## 59.5 A disabled module is absent, and says which one it is

`404` with `MODULE_UNAVAILABLE` and the module's id in the message — the existing error, not a new
code. 404 rather than 403 because the surface is not refused, it is **absent**: a client should not
go hunting a permission it will never be granted, and a crawler should not index a page that is one
config line from existing.

The gate is a layer on the module's whole router, mounted once in `server.rs`. Two properties are
asserted rather than assumed: the refusal is **per module** (a neighbour still answers in the same
test) and **not unconditional** (the same route answers `401` once the module is on).

## 59.6 Per-user state is a table, not a blob

`module_user_state (account_id, module_id, enabled, updated_at)`, one row per account per module a
reader has actually changed, no row meaning "inherit". Keyed on the **account**, not the pseud: a
reader who switches `arena` off once expects it off, and a per-pseud matrix of toggles is a
settings page nobody finishes. §46's rule applies — per-domain tables, never JSON blobs — and the
table's `account_id` needs a snapshot-policy decision (§11.16) like every other identity column.

## 59.7 The planned catalogue is registered, not implied

Every surface the roadmap has thought about and not built is a row with `stability: planned`,
`default_on: false`, no routes, and the milestone that owns it. They appear in `dh module list` and
in `/api/v1/modules`, and enabling one is **refused by name**:

```text
$ dh module check --config darkhold.toml
[modules] enabled names `home.hidden_gems`, which is planned (M64-12) and has no routes.
Enabling it would be claiming a feature this build does not have; building it is M64-12's job,
not a flag's.
```

A flag cannot compile a feature, and a catalogue that lists only what ships is how a roadmap
silently loses items. This is the same rule §0.3 applies to explanations: the absent thing says
what it is missing.

**As built (`2026-11-02`).** The registry's own header claimed that
`m59_extensions::every_route_module_is_registered` fails when a mounted surface has no row. **That
test did not exist**: `grep -rn every_route_module_is_registered crates/` found the sentence and
nothing else, so a surface could be mounted outside the mechanism and nothing would say so. The gate
now exists (`crates/app/tests/module_registry.rs`, 5 tests) and reads `server.rs` as text, because
the agreement under test is between two files:

- every `module_mount` id is in `CORE` or is a `Stability::Stable` row;
- every `Stable` row is mounted — a row for a surface nothing mounts advertises a feature that does
  not exist;
- a `Planned` row is **never** `default_on` and **never** `Toggle::Fixed`, so it can be turned on
  once built and is not offered to readers before then;
- the counts (`97` mount sites, `79` stable rows, `23` planned rows) are pinned, so a parser that
  stops matching fails instead of passing vacuously.

Two rules the first draft of the gate got wrong, both worth keeping: a module may legitimately be
mounted at several prefixes (`roadmap` three times, `monetization` three), so duplicate mount ids are
the design and not a fault; and the 23 dotted rows (`story.similar`, `home.trending`, …) are planned
slots, so "every row is mounted" would have deleted true rows to satisfy a false rule.

## 59.8 What an operator gets

- `dh module list [--enabled|--disabled|--planned] [--json]` — asks `/api/v1/modules`, and says so
  when it falls back to this build's compiled registry.
- `dh module show <id>` — one surface in full: state, toggle, dependencies, milestone, slots.
- `dh module check --config <path>` — the startup validation, without a server.
- `GET /api/v1/modules` — the public catalogue: id, name, summary, state, stability, toggle, nav.
  Not the operator's reasoning.
- `PATCH /api/v1/modules/{id}` — a reader's own override, `per_user` rows only.

## 59.10 Relevance decides the default, and the catalogue is generated

Two questions, one rule each.

**What is on for a fresh instance.** Every registry row carries `relevance` (0..10) and
`default_on`. A row that exists is 10 and on; a row that is **opt-in by design** is 4 and off, and
the list is short enough to name: `arena` (a calibration game), `quiz` (onboarding only), `tasting`
(an experimental sampler), `concierge` (an assistant answering on the instance's behalf),
`narration` (needs a speech engine on the host), `vanguard` (a role that means nothing until it is
granted). Nothing load-bearing is near the line: `works`, `search`, `catalogue`, `library`,
`reading` and `browse` are on, and a test asserts it — a formula that can switch off the catalogue
is a bug in the policy, not a leaner default.

**What is worth building next.** Every idea the project has had and not built is a registry row
with `stability: planned`, `default_on: false`, no routes, and a `relevance` that orders the queue:

| term | value |
|---|---|
| it has a `requirements.csv` row (work already agreed) | +4 |
| a spec amendment specifies it | +3 |
| an idea sweep or a design argues it | +2 |
| the 100-idea reader audit lists it | +1 |
| a vault note first typed it | +1 |
| named by title in this spec | +2 |
| appears in three or more sources | +1 |
| needs a third party or an external client (bot, mobile, desktop, OPDS, browser extension) | −2 |
| needs a model or heavy compute | −1 |
| the source hedges (maybe / someday / later) | −1 |

An idea is **never** enabled by its relevance, because a flag cannot compile a feature. Relevance
orders work; `stability` decides whether a switch exists. Enabling a planned row is refused by name,
quoting the documents it came from.

**The catalogue is generated**, not hand-listed, because the ideas live in eight places
(`docs/requirements.csv`, four brainstorms, three designs, thirteen spec amendments, the 100-idea
audit, named vault notes, and a superseded derived list) and a hand-kept list is a list somebody
stops updating the week they are busy. `scripts/ideas_catalogue.py` reads them and writes
`crates/domain/src/modules/ideas.rs` and `docs/ideas-catalogue.json`;
`the_idea_catalogue_matches_its_json` fails when the two disagree, so a hand edit that drops a row
is caught without regenerating.

Two extraction rules earned their place on the first run, which produced 375 rows that were mostly
furniture — "4 Acceptance", "Config", "Storage" — and sorted "5 Acceptance" above the brainstorm it
came from:

- **A spec amendment or a design is one idea** (its title). Its internal headings are its sections.
- **A brainstorm is a list of ideas**, so its `###` headings are the rows, and its `##` tier
  headings are not.

The script prints what it read, how many rows came from each source, and which built surfaces it
left off, so a run is auditable without opening the generated file.

Four sources are read as *idea lists* and the rest are not: a session note's headings are its
narration ("State at handoff", "Three decisions I made"), a requirements row keeps its own id
(`M63-12`, not a slug of its sentence), and a heading needs two word-ish tokens to count — the
audit's table has one-word cells that are attributes of a row, not items. Each of those rules was
paid for by reading a deployed payload that was 18 rows longer than it should have been.

## 59.12 The account layer narrows, and cannot do anything else

`module_user_state (account_id, module_id, enabled, updated_at)`, one row per account per module the
reader has **actually changed**. No row means inherit. `PATCH /api/v1/modules/{id}` is the only
write, and it can only narrow:

| request | effect |
|---|---|
| `{"enabled": false}` | writes a row; this reader no longer reaches the surface |
| `{"enabled": true}` | **deletes** the row; the instance's decision applies again — which may still be off |

Four rules the code enforces, each because the obvious alternative is a lie:

1. **Only a `per_user` row is the reader's to change.** An `operator` or `fixed` row answers
   `403 MODULE_NOT_USER_TOGGLEABLE` naming who decides (`operator`, `instance`). A `404` would say
   the surface does not exist, which sends the reader hunting for a settings page that will never
   appear.
2. **A planned row answers `422`**, because the request is well formed and the row has no routes —
   the objection is about work that does not exist yet, not about permission.
3. **The gate decides on the toggle, not on the row.** A row that exists for an `operator` surface —
   written by a migration, an older build or a mistaken administrator — silences nothing, and the
   catalogue does not report the surface as gone for that caller. `enabled_for_caller` uses the same
   test the gate uses, because two copies of one rule is how a page and a route come to disagree.
4. **A failed read fails open.** If the per-user lookup errors, the request continues and the failure
   is logged: a transient database error must not turn a surface into a `404` that reads as "this
   feature was removed".

The cost is one indexed primary-key read per request to a `per_user` surface, and only when the
request carries a session. The cache that would skip it — an instance-wide "does anyone hold an
override" flag — is deliberately not built (M65-09): it saves a row read and adds an invalidation
problem at startup, on write, and across processes.

## 59.11 The instance layer can re-enable an opt-in surface

`[modules] enabled = ["quiz", "arena"]` turns an opt-in-by-default surface back on for one instance,
and `the_same_route_is_not_a_404_when_the_surface_is_on` drives exactly that. This is what makes the
opt-in list safe to apply: a fresh instance is lean, and an instance that already has readers using
`arena` keeps it with one line, declared in the file rather than implied by a default.

## 59.9 Acceptance

1. `cargo test -p darkhold-app --test m65_module_host` green: a disabled module answers `404`
   `MODULE_UNAVAILABLE` naming itself; the same route answers `401` when enabled; neighbours are
   unaffected; the catalogue is core; an unregistered mount fails the build's own test; a `fixed`
   id and a typo both refuse with their reasons.
2. `cargo test -p darkhold-domain --lib modules` green (10 tests: uniqueness, defaults, resolution,
   the four refusals, the suggestion).
3. On a live instance, `[modules] disabled = ["quiz"]` makes `/api/v1/quiz/works` answer `404`
   `MODULE_UNAVAILABLE` naming `quiz`, and removing the line answers `401` — the difference between
   the two statuses is the whole proof.
4. `dh module check` refuses `moderation` and `quizz`, and accepts a valid file, with exit 0/2/2.
