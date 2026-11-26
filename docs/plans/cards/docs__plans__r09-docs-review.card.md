# docs/plans/r09-docs-review.md — plan card

Source: `docs/plans/r09-docs-review.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

Live Playwright at 1440×1000 and 390×844, index and `/docs/getting-started`:
- `Docs.svelte`: remove `<p class="summary">{doc.summary}</p>` from the detail branch; `fixLinks` assigns heading IDs and turns `.toc li` text into anchor links; `.toc li a` gets 44px flex rows; two-column `.index` above 60rem.
- Row becomes Done in `docs/reviews/page-review-queue.md` in the commit carrying the proof.

## Copy-pasteable code / commands

No fenced command block was recorded in the source document.

## Verification command and expected output

- Tests: `Docs.test.ts` asserts TOC links exist with `#` hrefs; `docs.test.ts` asserts summaries end at a word boundary.
- After expected: no duplicate intro, TOC anchors 44px, summaries with ellipsis, small detail `1/0` (estate select only) plus `kbd` on index, overflow 0; no Darkhold-owned errors.

## RED/GREEN evidence

No RED/GREEN evidence was recorded in the source document.
