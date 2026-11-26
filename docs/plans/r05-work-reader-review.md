# R-05 — published work page and reader: plan and acceptance

## Problem
Live Playwright at 1440×1000 and 390×844 on work `1d8eeae1-…` and its first unit:
- work detail renders the summary as literal `<p>Just trying out for fun, updates will be sporadic</p>` — a raw-HTML leak from importer data;
- metadata reads `5367 words · en` without grouping or human labels;
- breadcrumb/byline/CTA/chapter links are 19–43px tall;
- reader repeats the story title inside the imported body, and its Previous/Next and TOC links are 17px tall;
- reader search uses `var(--tap-min)`, which is 24px for fine-pointer browsers, so the measured input is 25px.

## Decision
Fix only Darkhold-owned rendering and CSS. Strip tags from the summary before rendering it as text (never render importer HTML on this page), group the word count, say `English`/`General audiences`, and give navigational links 44px targets. In the reader, drop the imported body's first heading when it repeats the work title, and make TOC/Previous/Next links 44px. Leave triple chapter headings, the malformed original-source URL and placeholder summaries alone: they are importer data, not this surface's contract.

## Changes
- `crates/app/src/routes/work_page.rs`: `plain_summary()` removes `<…>` and decodes the four common entities; `grouped_count()`; humanise language and rating; unit tests.
- `crates/app/src/routes/document.rs`: `.doc .site a, .doc .byline a, .doc .start-link, .doc .entries a` get `display:inline-flex; align-items:center; min-height:44px`.
- `crates/app/src/routes/reader_page.rs`: `normalize_imported_body(html, work_title)` drops a leading heading containing the work-title stem; caller passes `&work.title`; unit test extended.
- `frontend/src/styles/reader.css`: TOC and nav links get 44px min-height; search input gets a literal 44px min-height on touch rather than relying on `var(--tap-min)` for the measured case.

Execution note: the first pass left the chapter link at 22px (selector `.doc .contents a` missing) and the imported title as bare text outside a heading tag. Both were fixed in follow-up passes and now have unit tests plus live proof. The first-pass reader numbers were measured with fine-pointer browser contexts; the final phone context uses `has_touch=True, is_mobile=True`.

## Verification (before → after)
- Before: work small `4/4`, reader small `11/10`, raw `<p>` visible, `5367 words`, duplicated story title.
- After command: `r05_work_reader_audit.py` with phone contexts using `has_touch=True, is_mobile=True`, both widths, work and reader.
- After expected: no literal `<p>` in work body, `5,367 words · English`, work small `0/0`, reader small reduced to source-inline links only, no duplicated story title, no page errors.
- Targeted tests: `cargo test -p darkhold-app --lib r05`, `cargo test --test reader_surface`, `svelte-check` if CSS only.
- Row becomes Done in `docs/reviews/page-review-queue.md` in the commit carrying the before/after proof.
