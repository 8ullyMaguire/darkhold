# R-05 — published work page and reader live DOM review

Evidence: Playwright at 1440×1000 and 390×844 on work `1d8eeae1-cee2-41de-a5ac-d9f6c03741b8` and its first unit (`67e51d0a-…`), reached from the public landing page's `Start reading` link. Screenshots: `r05-work-1440.png`, `r05-reader-390.png` etc. DOM JSON: `r05-work-reader-audit.json`.

## GOOD
- Work page has one `h1`, a real `Start reading` CTA pointing at the first unit, and a chapter list; no overflow, no page errors at either width.
- Reader has one `h1` (`Chapter 1`), its own work link back, table of contents, Previous/Next, search-in-work, saved-place and bookmark controls; no overflow or JS page errors.
- Provenance is stated in plain language (`The text is here.`) and Discussion mode is labelled.
- The serif reading measure is comfortable and the document works without JavaScript.

## BAD
- Work summary rendered importer markup literally: `<p>Just trying out for fun, updates will be sporadic</p>`.
- Metadata read `general · in progress · 5367 words · en` — raw codes, ungrouped count.
- Work small controls `4/4`: breadcrumb 19px, byline 19px, CTA 43px, chapter link 22px.
- Reader small controls `11/10` with fine-pointer contexts: Previous/Next and all TOC links 17px; search input 25px because `--tap-min` is 24px outside coarse-pointer mode; source/FicHub inline links 17px.
- The imported body repeated the story title directly under the reader bar's copy of it.

## UGLY
- Every chapter heading appears three times (`chapter 2`, `CHAPTER 2`, `Chapter 2: First Flame`) — raw scraped-source structure, not a reader contract.
- `Original source` shows a malformed concatenated URL and a cryptic `FicHub.net` credit; the TOC says six entries while metadata says five chapters.
- “Your place in this unit is not known yet” leaks internal vocabulary; the placeholder summary `The text is here.` is provenance prose, not a blurb, but reads like a stub beside the raw markup.

## SUGGESTION (importer/data, not this row)
- Repair triple chapter headings, the source URL and the chapter-count mismatch at import time; never guess a source URL from a fragment client-side.
- Replace placeholder summaries at import time, and expose real chapter titles in the TOC where the importer has them.

## Fix in this row
Only Darkhold-owned rendering: strip importer tags from the summary before text output, group the count, humanise rating/language, give work/reader navigation links 44px targets, keep the search input 44px with a mouse, and drop a leading imported heading that repeats the work title. Triple headings and source URLs stay data SUGGESTION.

## After measurement
Appended when the row is closed.

## After measurement (deployed build)
- Final Vite exit `0`; final release build exit `0` in 6m 47s; `/` `200`. Work and reader both `200` at both widths.
- Work page: no literal `<p>` from the importer, body reads `General audiences · in progress · 5,367 words · English`, chapter line `Chapter 1 — 5,367 words`, one title occurrence, small controls `4 → 0` at both widths, overflow `0`, page errors `0`.
- Reader: title appears once (bar only) instead of twice; still one `h1` (`Chapter 1`), overflow `0`, page errors `0`. Remaining 10/9 "small" nodes are imported-body `<a>` anchors with no `href` (TOC artefacts in the source) and source/FicHub inline links, not navigation controls. The imported body's own source URL and triple chapter headings stay importer-data SUGGESTION.
- Targeted proof: `work_page::r05_tests` `2/2`, `reader_page::ui_tests` `4/4`, `reader_surface` `9/9`. Mutant replacing `&amp;` decoding with identity failed `left == right`; restored green.
- Two extra deploy passes were needed because the first CSS fix missed `.doc .contents a` and the first title-dedupe pass missed a bare-text title outside any heading tag; both are now covered by unit tests and live DOM measurements.
