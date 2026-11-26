# R-03 — `/discover` review: plan and acceptance

## Problem
Live Playwright at 1440×1000 and 390×844 found zero overflow and zero small controls, but the feed itself failed its own introduction:
- every row's monospace eyebrow repeated the bold title verbatim (`Fox Hole 159 · by devwriter` above `Fox Hole 159`), doubling row height for no information;
- no word count, tags or update date was shown even though `DiscoveryItem.word_count` is already on the wire;
- titles were bold black with no link affordance, and the feed stopped with no end-of-feed line;
- `Sort: For you` names a mode as a sort order;
- desktop left ~500px of dead space because the content was a narrow single column.

The all-one-author feed and repost duplicates are live data, not UI defects; they are recorded as SUGGESTION rather than invented away.

## Decision
Fix only Darkhold-owned SPA code. The eyebrow becomes `by <handle> · <formatted> words` from existing fields; the title becomes an underlined accent link; a closing line states that the feed ended; the shared sort control takes an optional label and Discover says `Order`. Do not add pagination (the API has no cursor) or filter out reposts by guessing what a reader wants.

## Changes
- `frontend/src/routes/Discover.svelte`: add a local thousands formatter; replace the eyebrow expression; add `.feed-end`; underline/colour `.feed-title`; pass `label="Order"` to `SortControl`.
- `frontend/src/lib/components/SortControl.svelte`: optional `label` prop defaulting to `Sort`, used by the `<label>`.
- `frontend/src/routes/Discover.test.ts`: assert the eyebrow is exactly `by alice · 3,000 words` and the title is no longer repeated in it.
- `frontend/src/lib/components/SortControl.test.ts`: assert the Discover label override renders.

Execution note: the first pass used `word_count` for the eyebrow, but the live `/api/v1/discovery` response carries `reason`, `reason_code` and `match_percent` and no word count. The type and eyebrow were corrected to use the fields that actually exist; the deployed proof shows `Readers have rated it highly. · by devwriter · 100% match`. A post-restart `HOME=000` was a warm-up timing artefact: local health was `200` and a direct public re-probe was `200` in 1.4s.

## Verification (before → after)
- Before: `.feed-work` = `Discovery Test · by alice`; no end line; label `Sort`.
- After command: `r03_discover_audit.py` at both widths after deploy.
- After expected: `.feed-work` = `by devwriter · 5,367 words` (or the live count), title appears once per row as an underlined link, an end-of-feed line exists, label `Order`, overflow `0`, small `0`, no Darkhold page errors.
- Targeted tests: `Discover.test.ts`, `SortControl.test.ts`, `svelte-check`.
- Row becomes Done in `docs/reviews/page-review-queue.md` in the same commit whose message names R-03 and whose body carries the before/after line.
