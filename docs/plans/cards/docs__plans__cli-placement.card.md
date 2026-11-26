# docs/plans/cli-placement.md — plan card

Source: `docs/plans/cli-placement.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

**Decision: in this workspace, as `crates/cli` (binary `dh`), with the TUI as `dh tui` in the same
Written after the power-user sweep (`docs/ideas/2026-11-01-idea-sweep-power-users.md`), which put
| **ascend** | builds/decks | works, readers | a **file**: a seed out, a URL claim in (`docs/NETWORK.md`) |
- `docs/plans/bots-extension.md` §2 on Lorebook: *"Do not add Lorebook to the Darkhold workspace.
- `docs/plans/bots-extension.md` §1 on the bot: a consumer **compiles against a shared contract**,
  never against the server (`crates/metadata` is the precedent).
1. **`crates/cli` may not depend on `crates/app` or `crates/db`.** It depends on the wire types
   (`crates/metadata`, `crates/domain`) and `reqwest`; if it needs more than that, the contract is
   missing a type and the type belongs in `crates/metadata`, not in an import.
window grammar exists to prevent (`docs/ideas/2026-11-01-idea-sweep-power-users.md`, "The window
SQLite snapshot of the catalogue with no reader in it (implemented; `crates/db/src/catalogue_export.rs`).
2. `crates/cli` with `dh rec`, `dh q`, `dh export` — the three verbs that need nothing but the API

## Copy-pasteable code / commands

No fenced command block was recorded in the source document.

## Verification command and expected output

the two would drift, and the drift would be silent because nothing in Lorebook's test suite talks
`cargo test --workspace` fails on the day a route's shape changes. In a separate repo, the first

## RED/GREEN evidence

No RED/GREEN evidence was recorded in the source document.
