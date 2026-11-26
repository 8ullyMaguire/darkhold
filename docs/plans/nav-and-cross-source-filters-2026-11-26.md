# New task plan — 2026-11-26

## 1. More-menu overflow
- Component: `frontend/src/routes/../lib/components/NavMenu.svelte`.
- Current `place()` anchors a `position: fixed` panel at `rect.bottom + 8`; the `$effect` clamps only horizontal position. A low trigger plus a tall panel can put `top + height` past `window.innerHeight`.
- Fix: extract a pure `clampPanel({left, top}, width, height, viewportWidth, viewportHeight, margin)` that clamps both axes, with `Math.max(margin, ...)` so a panel taller than the viewport still starts on-screen. Use it in the existing effect; add `max-height: calc(100vh - 2 * margin)` and `overflow-y: auto` to `.panel.floating` as a belt-and-braces guard.
- Tests: pure clamp cases (bottom overflow, tiny viewport, unchanged inside viewport) in `NavMenu.test.ts`; live DOM probe at 1440x700 and 390x600 measuring panel `getBoundingClientRect()` after clicking More.

## 2. Cross-source fanfic filter test
- Query under test: `(tag:cyoa OR tag:apocalypse OR tag:reincarnation OR tag:isekai OR tag:"time travel") words:>30000 language:en source_updated:<=-3m` with no source filter.
- Live API already returned 4 pawchive hits, e.g. `Star Forge` 161,259 words; the same tag/word/language query without the stale filter returned 100 hits across `archive` (96) and `pawchive` (4). So the grammar and normalized metadata are already capable; coverage of `source_updated_at` varies by source (ao3 55/2995, archive 0/28488, pawchive 26655/26858), which is a data-coverage limitation, not a query bug.
- Add a dual-engine DB test `crates/db/tests/cross_source_tag_filters.rs` seeding two works from different `source_key`s with tags, word counts, language and stale source dates, then asserting the AST search returns both sources for the exact query and excludes a fresh work. This proves the capability independent of current live coverage.
- Record the exact live result and the source-date coverage in `docs/plans/followup-suggestions-2026-11-26.md` and `docs/handoff.md`.
