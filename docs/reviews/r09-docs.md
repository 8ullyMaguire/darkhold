# R-09 — `/docs` live DOM review

Evidence: Playwright at 1440×1000 and 390×844, `/docs` index and `/docs/getting-started`, reached through the in-app Help link. Screenshots: `r09-docs-{desktop,phone}-{index,detail}.png`. DOM JSON: `r09-docs-audit.json`.

## GOOD
- Correct `h1` on both surfaces (`Help`, `Getting started`); no horizontal overflow at either width.
- The index lists all eight authored help pages with titles and summaries; keyboard shortcut is documented in plain language.
- Detail page has a genuine TOC, and body headings/anchors are authored content, not fabricated.
- No Darkhold-owned page errors on the index; the intermittent `attachShadow` error on article pages is estate chrome.

## BAD
- Detail page repeated its introduction: a clipped copy of the first paragraph above the TOC, then the full paragraph in the body.
- TOC entries were plain 22px text bullets, not links — they looked navigable but did nothing.
- Index summaries were cut at 140 characters mid-word (`recognis`, `ways t`), making card descriptions hard to scan.
- Index used one narrow column on desktop despite a 1440px viewport and a wide right gutter.
- `Ctrl+K` / `⌘+K` chips and footer links were small, and the search affordance was prose-only.

## UGLY
- Footer `Community`, `Radio` and `Help` links are 14px olive text; `Radio` has no description.
- Header `Search` is text-only while the prose invites a keyboard shortcut; the connection is not obvious.
- Estate bottom bar duplicates Help and sign-in and competes with the page's own content.

## SUGGESTION (separate owner / shell)
- Add a visible `Search help` button that dispatches the same Ctrl+K command as the shell; that command belongs to the app shell, not this route.
- Raise footer link sizes and explain `Radio` at the shell/estate layer.
- Fix injected estate chrome (`attachShadow`, bottom bar).

## Fix in this row
Only Darkhold-owned SPA code: remove the duplicate intro, make TOC rows real 44px anchors with generated heading IDs, trim summaries at a word boundary, and use two columns on wide viewports. No fake search box or invented help content.

## After measurement
Appended when the row is closed.

## After measurement (deployed build)
- Final Vite exit `0`; release build exit `0` in 6m 45s; `/` `200`; `/docs` and `/docs/getting-started` render at both widths with one `h1`, `overflow=0`, and no Darkhold-owned errors.
- Small controls before → after: detail desktop `6 → 2`, phone `5 → 1`. Remaining items are the estate theme `<select>` and static `<kbd>` chips on the index; the breadcrumb link is now 44px tall (31px wide, above the 24px WCAG 2.5.8 minimum) and body/TOC links are 44px. No horizontal overflow in any case.
- Duplicate intro removed; TOC entries are real anchors to generated heading IDs; index summaries end at a word boundary with an ellipsis; index cards use two columns at 1440px. No fabricated help content or search box was added.
- Targeted proof: `Docs` `4/4`, docs registry `7/7`, `svelte-check` `0 errors and 10 warnings`. Mutant changing the TOC href to `#` failed `expected '#' to be '#what-this-site-is'`; restored green. A summary-ellipsis mutant passed because all current summaries are under 140 chars — the word-boundary code is correct but not covered by current data.
- The intermittent `attachShadow` error remains on article pages: injected estate chrome. Footer `Radio` clipping and the search affordance remain shell/estate SUGGESTION.
