# docs/plans/r03-discover-review.md — plan card

Source: `docs/plans/r03-discover-review.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

- `frontend/src/routes/Discover.svelte`: add a local thousands formatter; replace the eyebrow expression; add `.feed-end`; underline/colour `.feed-title`; pass `label="Order"` to `SortControl`.
- `frontend/src/lib/components/SortControl.svelte`: optional `label` prop defaulting to `Sort`, used by the `<label>`.
- `frontend/src/routes/Discover.test.ts`: assert the eyebrow is exactly `by alice · 3,000 words` and the title is no longer repeated in it.
- `frontend/src/lib/components/SortControl.test.ts`: assert the Discover label override renders.
- Row becomes Done in `docs/reviews/page-review-queue.md` in the same commit whose message names R-03 and whose body carries the before/after line.

## Copy-pasteable code / commands

No fenced command block was recorded in the source document.

## Verification command and expected output

- `frontend/src/routes/Discover.svelte`: add a local thousands formatter; replace the eyebrow expression; add `.feed-end`; underline/colour `.feed-title`; pass `label="Order"` to `SortControl`.
- `frontend/src/routes/Discover.test.ts`: assert the eyebrow is exactly `by alice · 3,000 words` and the title is no longer repeated in it.
- `frontend/src/lib/components/SortControl.test.ts`: assert the Discover label override renders.
Execution note: the first pass used `word_count` for the eyebrow, but the live `/api/v1/discovery` response carries `reason`, `reason_code` and `match_percent` and no word count. The type and eyebrow were corrected to use the fields that actually exist; the deployed proof shows `Readers have rated it highly. · by devwriter · 100% match`. A post-restart `HOME=000` was a warm-up timing artefact: local health was `200` and a direct public re-probe was `200` in 1.4s.
- Before: `.feed-work` = `Discovery Test · by alice`; no end line; label `Sort`.
- After expected: `.feed-work` = `by devwriter · 5,367 words` (or the live count), title appears once per row as an underlined link, an end-of-feed line exists, label `Order`, overflow `0`, small `0`, no Darkhold page errors.
- Targeted tests: `Discover.test.ts`, `SortControl.test.ts`, `svelte-check`.

## RED/GREEN evidence

Live Playwright at 1440×1000 and 390×844 found zero overflow and zero small controls, but the feed itself failed its own introduction:
