# R-01 — `/` landing page, live DOM review

Evidence: Playwright at 1440×1000 and 390×844, `r01_landing_audit.py`, 2026-11-26.
Screenshots: `r01-desktop.png`, `r01-phone.png`. Live DOM JSON: `r01-landing-audit.json`.

## GOOD
- One `h1` (`Darkhold`) at both widths, sensible order: identity → tagline → catalogue count → search → reading list → what-you-can-do.
- Zero page errors, zero images missing `alt`, zero horizontal overflow: `scrollWidth` equals viewport at both widths (`1440/1440`, `390/390`).
- The front door works without script: the form posts to `/search`, and starter titles link to server-rendered reader units when readable.
- Serif/sans/monospace mix is a deliberate archive voice; the rust link colour is consistent.

## BAD
- `141939 works catalogued from 11 sources` is an ungrouped digit blob.
- Story metadata is one undifferentiated monospace run; at 390px `general` orphaned alone on its own line.
- `Sign in · Help` sat stranded between the rule and the estate footer and duplicated the estate's Sign in pill.
- Body links `Browse the catalogue`, `Open the editor`, `Browse the forum` measured 22px tall — below the 44px target the queue asks for.
- The Search button was a plain outline with no accent fill, making the primary action look secondary.

## UGLY
- The landing footer's `Sign in` text link and the estate bar's Sign in pill sat 20px apart with no shared grouping — it read as two competing navigations.
- On phone the story list's rating wrapping onto a bare `general` line made the metadata look broken rather than dense.

## SUGGESTION (estate owner)
- The injected Polaris wordmark renders white on cream and is effectively invisible; the globe/scales/Discord icon buttons have no labels or tooltips. Both belong to the estate chrome, not this document. Fix in the injected bar, not by a darkhold-only CSS override.
- The large cream gap below the footer on both widths reads as an unfinished page; consider a genuine bottom boundary or removing the fixed minimum height.

## Fix in this row
Only document-owned defects: grouped counts, two-part metadata groups, 44px body links, accented Search button, one Sign in door, `Start reading` heading, and a `Read the docs` link in the document.
Before: `small=7` desktop / `small=3` phone, `141939`, duplicated `Sign in`, orphan `general`.
After measurement is appended to `docs/reviews/page-review-queue.md` when the row is closed.

## After measurement (deployed build)
- `/` `200`; build exit `0`, `Finished release profile ... in 7m 24s`.
- Desktop: `h1=Darkhold`, `scroll=1440/1440`, `overflow=0`, `errors=0`, `small=2`.
- Phone: `h1=Darkhold`, `scroll=390/390`, `overflow=0`, `errors=0`, `small=0`.
- Body now reads `141,940 works catalogued from 11 sources`, `by devwriter · 5,367 words`, `in progress · general`, `Start reading`, and `Need the details? Read the docs.`.
- The two remaining desktop "small" links are the story titles at `652×28`. That is a large-area, single-line link; 28px height exceeds WCAG 2.5.8's 24px minimum. Raising them to 44px would inflate every shelf row, so it is deliberately not done.
- Targeted proof: `counts_are_grouped_for_reading` `1 passed`; `crates/app/tests/landing_page.rs` `5 passed`; mutant `out.push(',')` → `out.push(' ')` failed the unit test, restored.
