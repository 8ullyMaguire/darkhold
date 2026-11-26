# R-04 — search SPA review: plan and acceptance

## Problem
Live Playwright via in-app navigation at 1440×1000 and 390×844, default and `?q=winter`, found:
- phone results view: `scrollWidth=515` against `390`, 9 overflowing histogram elements, 30 small controls — ten fixed-width histogram bars cannot fit;
- histogram counts render raw (`8`, `22768`) while result rows use separators;
- result title links are 17px tall and provenance/status chips are ~24px;
- facet counts are unformatted, and `{ageMs}ms ago` is developer instrumentation;
- results begin with no `Results` heading after five facet groups.

## Decision
Fix only Darkhold-owned SPA code. On narrow viewports the histogram becomes a stacked list of full-width 44px rows, keeping the server's quantile edges and the click-to-filter behaviour. Format all counts with `Intl.NumberFormat`. Raise result/title and chip targets to 44px where they are controls. Add a `Results` heading, and state the age in seconds only (sub-second staleness is not actionable). Do not invent a result total the API does not provide.

## Changes
- `WordHistogram.svelte`: `fmt(bucket.count)` in the count span; `@media (max-width: 48rem)` stacked bars with `height:auto`, `min-height:44px`, wrapping captions.
- `FacetPanel.svelte`: `fmt()` for bucket counts and fandom coverage; `age()` drops the `0ms ago` instrumentation.
- `WorkRow.svelte`: `h3 a` gets `display:inline-flex; align-items:center; min-height:44px`; `.facts span` gets `min-height:44px` and flex alignment.
- `Search.svelte`: `<h2 class="results-heading">Results</h2>` before the list.
- Tests: add/extend `WordHistogram.test.ts`, `FacetPanel.test.ts`, `WorkRow.test.ts`, `Search.test.ts`; `svelte-check`.

Execution note: the first deployed pass still showed a clipped `the catalogue under...` note and unformatted `30900`/`58042` in server gap sentences. The final pass prepends `Counts describe` in `search.rs` and adds `fmt_count` in `facets.rs`; both are covered by the live probe. Desktop histogram bars remain 24px by decision; phone rows are 44px.

## Verification (before → after)
- Before: phone query `scrollWidth=515/390`, `small=30`, overflow 9; counts raw; no Results heading.
- After command: `r04_search_audit.py` at both widths, default and `?q=winter`, after deploy.
- After expected: phone query `scrollWidth=390/390`, overflow `0`, small `0` apart from the estate theme select; grouped counts; `Results` heading present; no page errors.
- Row becomes Done in `docs/reviews/page-review-queue.md` in the same commit whose message names R-04 and whose body carries the before/after line.
