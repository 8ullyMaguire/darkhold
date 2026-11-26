# R-09 — docs pages: plan and acceptance

## Problem
Live Playwright at 1440×1000 and 390×844, index and `/docs/getting-started`:
- detail page repeats its intro: a summary paragraph clipped mid-word above the TOC, then the same paragraph complete in the body;
- TOC entries are plain `<li>` text, not links, and 22px tall;
- index card summaries are cut at 140 characters mid-word;
- index uses one narrow column on desktop with a large right gutter;
- `kbd` chips and footer links are small; intermittent `attachShadow` error is estate chrome.

## Decision
Fix only Darkhold-owned SPA code. Remove the duplicate summary from the detail page. Give TOC entries real anchors with generated heading IDs and 44px rows. Trim index summaries at a word boundary with an ellipsis, and use two columns on wide viewports. Do not add a fake search input or invent shortcuts; the real Ctrl+K search belongs to the app shell.

## Changes
- `Docs.svelte`: remove `<p class="summary">{doc.summary}</p>` from the detail branch; `fixLinks` assigns heading IDs and turns `.toc li` text into anchor links; `.toc li a` gets 44px flex rows; two-column `.index` above 60rem.
- `lib/docs.ts`: `summaryOf()` cuts at the last word boundary before 140 chars and appends `…`.
- Tests: `Docs.test.ts` asserts TOC links exist with `#` hrefs; `docs.test.ts` asserts summaries end at a word boundary.

Execution note: after the first deployed audit, body links and the shared breadcrumb were still 22px; a second scoped `:global()` patch raised them to 44px and the final measurement above is from that build. Two builds were required because the frontend assets are embedded in the Rust binary.

## Verification (before → after)
- Before: index small desktop/phone `5/4`; detail `6/5`; duplicate intro and 22px TOC rows; index summaries mid-word.
- After command: `r09_docs_audit.py` on index and detail at both widths.
- After expected: no duplicate intro, TOC anchors 44px, summaries with ellipsis, small detail `1/0` (estate select only) plus `kbd` on index, overflow 0; no Darkhold-owned errors.
- Row becomes Done in `docs/reviews/page-review-queue.md` in the commit carrying the proof.
