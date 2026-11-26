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

## Final evidence

- More menu at 1440×700 on `/discover`: trigger bottom 56, old unclamped panel top 64 and bottom 748 (48px past the viewport). After the fix the panel measured `y=8, height=684, bottom=692` — fully on-screen. The dialog's Search help button measured 44px high and opening it produced the Search help pages input. Footer Radio now reads `Radio / Shortwave episodes in a tab`.
- `NavMenu` 16/16, `App` 15/15, `Docs` 5/5, `svelte-check` 0 errors and 10 warnings in 6 files. Live page errors: 0 Darkhold errors; the only page error is the known estate `attachShadow` duplicate from the separately owned injected topbar.
- Cross-source dual-engine test 1/1 on SQLite and 1/1 on PostgreSQL; fresh-clock mutant admitted the third work and was restored.
- AO3 live backfill after deploy of `c99dc67a`: 2,082 works, 52,382 assertions, 11,439 new canonical nodes; probe work `004327ce-eeaa-4083-9217-be1f40dd4af1` went from 0 to 54 tags.
