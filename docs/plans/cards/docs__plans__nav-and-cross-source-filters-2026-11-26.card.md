# docs/plans/nav-and-cross-source-filters-2026-11-26.md — plan card

Source: `docs/plans/nav-and-cross-source-filters-2026-11-26.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

- Component: `frontend/src/routes/../lib/components/NavMenu.svelte`.
- Add a dual-engine DB test `crates/db/tests/cross_source_tag_filters.rs` seeding two works from different `source_key`s with tags, word counts, language and stale source dates, then asserting the AST search returns both sources for the exact query and excludes a fresh work. This proves the capability independent of current live coverage.
- Record the exact live result and the source-date coverage in `docs/plans/followup-suggestions-2026-11-26.md` and `docs/handoff.md`.

## Copy-pasteable code / commands

No fenced command block was recorded in the source document.

## Verification command and expected output

- Tests: pure clamp cases (bottom overflow, tiny viewport, unchanged inside viewport) in `NavMenu.test.ts`; live DOM probe at 1440x700 and 390x600 measuring panel `getBoundingClientRect()` after clicking More.
## 2. Cross-source fanfic filter test
- Query under test: `(tag:cyoa OR tag:apocalypse OR tag:reincarnation OR tag:isekai OR tag:"time travel") words:>30000 language:en source_updated:<=-3m` with no source filter.
- Add a dual-engine DB test `crates/db/tests/cross_source_tag_filters.rs` seeding two works from different `source_key`s with tags, word counts, language and stale source dates, then asserting the AST search returns both sources for the exact query and excludes a fresh work. This proves the capability independent of current live coverage.
- Cross-source dual-engine test 1/1 on SQLite and 1/1 on PostgreSQL; fresh-clock mutant admitted the third work and was restored.

## RED/GREEN evidence

- Cross-source dual-engine test 1/1 on SQLite and 1/1 on PostgreSQL; fresh-clock mutant admitted the third work and was restored.
