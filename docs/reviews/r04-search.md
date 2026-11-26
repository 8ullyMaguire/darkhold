# R-04 — `/search` SPA live DOM review

Evidence: Playwright at 1440×1000 and 390×844, via in-app navigation (desktop header link; phone `More` drawer), default and `?q=winter`, `r04_search_audit.py`, 2026-11-26. Screenshots: `r04-search-{desktop,phone}-{default,query}.png`. DOM JSON: `r04-search-audit.json`.

## GOOD
- One `h1` (`Search works`) and no page errors in any of the four cases.
- The query state is carried in the URL (`/search?q=winter`), so a result can be shared or reloaded.
- Facet groups are real counts under the stated filter set, with explicit measured/unknown split; the histogram is computed server-side from the same set.
- Zero-result rescue, saved searches, scoped search and include/exclude chips are all present on the page — the mechanics shipped in earlier rows are wired into this surface.

## BAD
- Phone results view: `scrollWidth=515` against `390`, 9 overflowing histogram elements and 30 small controls. Ten fixed-width quantile columns cannot fit.
- Histogram counts rendered raw (`8`, `22768`) while result rows formatted `3,122 words`.
- Result title links were 17px tall; provenance/status chips ~24px.
- Facet counts (`58042`, `55913`) were unformatted, and `Counts from 0ms ago` is instrumentation leaking into user-facing prose.
- The phrase beneath the histogram clipped mid-sentence on phone, and facet groups ran straight into results with no `Results` heading.

## UGLY
- Every visible hit was `by dewwriter` with `0 points`, while the subtitle promises character/relationship/tag/mood search. That is a live-data credibility problem, not a UI bug.
- "Hosted here" and "Live" appeared on every row with no legend; the two outlined pills read as noise.
- The `Any fandom` text beside the disabled `Add` button looked like a control that had lost its state.

## SUGGESTION (data/operator, not this row)
- Fix the single-author/zero-points feed at import and ranking level, and surface match context on rows only when the server records it.
- Give provenance/status badges a one-line legend or merge them; do not invent semantics client-side.

## Fix in this row
Only Darkhold-owned SPA code: stacked histogram rows on narrow screens, grouped counts everywhere, an age that says `now` rather than `0ms ago`, 44px result links, and a `Results` heading.
Before: phone `scrollWidth=515/390`, `small=30`, overflow `9`; raw counts; no Results heading.
After measurement is appended to `docs/reviews/page-review-queue.md` when the row is closed.

## After measurement (deployed build)
- Vite exit `0`; final release build exit `0` in 8m 43s; `/` `200`; `/search?q=winter` `200`.
- Phone query: `scrollWidth=390/390`, `overflow=0`, `small=0`, zero Darkhold page errors. Before: `515/390`, `9` overflowing bars, `30` small controls.
- The histogram is now ten stacked, full-width rows; counts read `8`, `9`… under labels like `99,691 – 366,699 words`, with `85 measured of 86` and the gap sentence intact. Facet counts read `58,042`, `22,768`, `30,900 of 58,042`; the age reads `Counts: 4.8s ago` instead of `Counts from 0ms ago`.
- The note now reads `Counts describe the catalogue under min_words, max_words and max_rating — not this page of hits` — a full sentence, not a clipped fragment.
- A `Results` heading sits directly before the work list. Result title links are at least 44px tall; desktop histogram bars remain 24px tall because they are a chart with proportional heights, and WCAG 2.5.8's 24px minimum is met — forcing 44px would flatten the distribution.
- Targeted proof: `WordHistogram` `4/4`, `FacetPanel` `5/5`, `WorkRow` `3/3`, `Search` `31/31` (43 total), `svelte-check` `0 errors and 10 warnings`. Mutant restoring the raw histogram count failed; mutant removing the thousands separator from `fmt_count` failed `left == right`; both restored.
