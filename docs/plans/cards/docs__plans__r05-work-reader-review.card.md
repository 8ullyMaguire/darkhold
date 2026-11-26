# docs/plans/r05-work-reader-review.md — plan card

Source: `docs/plans/r05-work-reader-review.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

- `crates/app/src/routes/work_page.rs`: `plain_summary()` removes `<…>` and decodes the four common entities; `grouped_count()`; humanise language and rating; unit tests.
- `crates/app/src/routes/document.rs`: `.doc .site a, .doc .byline a, .doc .start-link, .doc .entries a` get `display:inline-flex; align-items:center; min-height:44px`.
- `crates/app/src/routes/reader_page.rs`: `normalize_imported_body(html, work_title)` drops a leading heading containing the work-title stem; caller passes `&work.title`; unit test extended.
- `frontend/src/styles/reader.css`: TOC and nav links get 44px min-height; search input gets a literal 44px min-height on touch rather than relying on `var(--tap-min)` for the measured case.
- Row becomes Done in `docs/reviews/page-review-queue.md` in the commit carrying the before/after proof.

## Copy-pasteable code / commands

No fenced command block was recorded in the source document.

## Verification command and expected output

- `crates/app/src/routes/reader_page.rs`: `normalize_imported_body(html, work_title)` drops a leading heading containing the work-title stem; caller passes `&work.title`; unit test extended.
Execution note: the first pass left the chapter link at 22px (selector `.doc .contents a` missing) and the imported title as bare text outside a heading tag. Both were fixed in follow-up passes and now have unit tests plus live proof. The first-pass reader numbers were measured with fine-pointer browser contexts; the final phone context uses `has_touch=True, is_mobile=True`.
- After expected: no literal `<p>` in work body, `5,367 words · English`, work small `0/0`, reader small reduced to source-inline links only, no duplicated story title, no page errors.
- Targeted tests: `cargo test -p darkhold-app --lib r05`, `cargo test --test reader_surface`, `svelte-check` if CSS only.

## RED/GREEN evidence

No RED/GREEN evidence was recorded in the source document.
