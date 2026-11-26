# R-07 — roadmap page: plan and acceptance

## Problem
Live Playwright at 1440×1000 and 390×844, Board / Arena / Changelog tabs:
- `Sign in` (22px) and `vote in the arena` (22px) are inline links in the board empty state; arena `Sign in` is also 22px.
- Active tab is a filled rust pill, visually identical to a CTA, and inactive tabs are small text.
- Board empty state is left-aligned in a narrow column with a large void; no visual container or primary action.
- Arena empty state is one small sentence in a void.

## Decision
Fix only Darkhold-owned SPA code. Restyle tabs as tabs: 44px tall, active state as an underline, not a filled button. Give the board empty state a centred, bordered panel with a primary `Sign in to suggest` button and a secondary `vote in the arena` link; give Arena a primary `Sign in to vote` button. Do not invent feature cards or ballot data — the empty board is real live data. Header/estate sign-in duplicates remain separate-owner SUGGESTION.

## Changes
- `Roadmap.svelte`: tab CSS (`min-height:44px`, underline active), `.board-empty` centred panel, `.empty-cta`/`.arena-signin .button` with 44px links and accent fill, arena copy updated.
- `Roadmap.test.ts`: extend empty-state test to assert the two CTA links with classes and the tab active class; targeted Vitest + `svelte-check`.

Execution note: `svelte-check` passed with 0 errors; the arena test needed its text assertion retargeted from the old inline sentence to the new `Sign in to vote` button. The footer links and estate bar are deliberately out of this row.

## Verification (before → after)
- Before: desktop small `3/2/1` across tabs, phone `2/1/0`; board links 22px; arena link 22px.
- After command: `r07_roadmap_audit.py` in both widths, all three tabs.
- After expected: board small desktop `1` (estate select), phone `0`; arena/changelog `1/0`; no overflow, no page errors; tabs 44px; primary CTA visible.
- Row becomes Done in `docs/reviews/page-review-queue.md` in the commit carrying the proof.
