# docs/plans/m65-module-host.md — plan card

Source: `docs/plans/m65-module-host.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

**Spec:** §59 (`docs/spec.md`). **Closes:** the first-party half of §6.3's slot promise.
`/api/v1/extensions` **already exists**: `marketplace.rs` serves a third-party extension catalogue
`GET /extensions` in `routes/extensions.rs` and the assembled router panicked with
| mounted in `server.rs` | **79** across **90** merge sites | the sweep is mechanical, and a script can do all of it |
| `config.rs` | **6,160 lines**, **49** top-level sections | a new section is the cheapest possible config surface; the file already carries 49 |
| `docs/design/0004-module-marketplace-architecture.md` | **absent** | it is referenced by the spec's design index and does not exist — recorded, not fixed here |
registry cannot drift from `server.rs` without the door test failing. `Stability::Planned` rows
compiled default → darkhold.toml [modules] → account override → resolved
Generated from `server.rs` by parsing the 90 merge sites; a module missing from this table fails
1. `crates/app/src/ext/mod.rs` — `Extension`, `Stability`, `Toggle`, `NavEntry`, `REGISTRY`,
2. `crates/app/src/ext/registry.rs` — the 74 rows above + planned rows.
3. `crates/app/src/ext/config.rs` — `ExtensionsConfig { disabled, enabled, allow_user_overrides }`

## Copy-pasteable code / commands

pub struct Extension {
    pub id: &'static str,
    pub name: &'static str,
    pub summary: &'static str,
    pub default_on: bool,
    pub stability: Stability,   // Stable | Beta | Experimental | Planned
    pub toggle: Toggle,         // Fixed | Operator | PerUser
    pub requires: &'static [&'static str],
    pub milestone: &'static str,
    pub spec: &'static str,
    pub nav: &'static [NavEntry],
}

compiled default → darkhold.toml [modules] → account override → resolved

## Verification command and expected output

itself at test time, one merge away from a live 500.
registry cannot drift from `server.rs` without the door test failing. `Stability::Planned` rows
4. `crates/app/src/ext/gate.rs` — `mount(state, id, router, class)`; `AppState::modules`.
**Gate:** `cargo test -p darkhold-app --test m65_module_host` green (7 tests: disabled 404 names the
`cargo test -p darkhold-app --lib` green; live: `curl /api/v1/quiz/...` → 404 `module_disabled`
3. `crates/app/src/ext/gate.rs` — when the module is `per_user` **and** any account holds an
**Gate:** `cargo test -p darkhold-db --test module_state` 4 tests both engines; a route test
gate. Relevance (§59.10) orders the queue and, for built surfaces, decides the default: six are
fresh instance is 68 on / 29 off, and `[modules] enabled` turns one back on — the m65 test drives

## RED/GREEN evidence

**Gate:** `cargo test -p darkhold-app --test m65_module_host` green (7 tests: disabled 404 names the
`cargo test -p darkhold-app --lib` green; live: `curl /api/v1/quiz/...` → 404 `module_disabled`
