# docs/plans/r04-search-review.md — plan card

Source: `docs/plans/r04-search-review.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

- `WordHistogram.svelte`: `fmt(bucket.count)` in the count span; `@media (max-width: 48rem)` stacked bars with `height:auto`, `min-height:44px`, wrapping captions.
- `FacetPanel.svelte`: `fmt()` for bucket counts and fandom coverage; `age()` drops the `0ms ago` instrumentation.
- `WorkRow.svelte`: `h3 a` gets `display:inline-flex; align-items:center; min-height:44px`; `.facts span` gets `min-height:44px` and flex alignment.
- `Search.svelte`: `<h2 class="results-heading">Results</h2>` before the list.
Execution note: the first deployed pass still showed a clipped `the catalogue under...` note and unformatted `30900`/`58042` in server gap sentences. The final pass prepends `Counts describe` in `search.rs` and adds `fmt_count` in `facets.rs`; both are covered by the live probe. Desktop histogram bars remain 24px by decision; phone rows are 44px.
- Row becomes Done in `docs/reviews/page-review-queue.md` in the same commit whose message names R-04 and whose body carries the before/after line.

## Copy-pasteable code / commands

No fenced command block was recorded in the source document.

## Verification command and expected output

- Tests: add/extend `WordHistogram.test.ts`, `FacetPanel.test.ts`, `WorkRow.test.ts`, `Search.test.ts`; `svelte-check`.
Execution note: the first deployed pass still showed a clipped `the catalogue under...` note and unformatted `30900`/`58042` in server gap sentences. The final pass prepends `Counts describe` in `search.rs` and adds `fmt_count` in `facets.rs`; both are covered by the live probe. Desktop histogram bars remain 24px by decision; phone rows are 44px.
- After expected: phone query `scrollWidth=390/390`, overflow `0`, small `0` apart from the estate theme select; grouped counts; `Results` heading present; no page errors.

## RED/GREEN evidence

No RED/GREEN evidence was recorded in the source document.
