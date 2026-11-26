# R-08 — community page: plan and acceptance

## Problem
Live Playwright at 1440×1000 and 390×844, signed out and signed in (temporary staff session, deleted after use):
- signed-out gate was a 22px inline `Sign in` link in otherwise empty space, with no panel or primary action;
- signed-in Forums filter input was 21px tall and its `Search posts` link was 22px, with no visible gap or accessible name on the input;
- Forum/Messages/Blocks/Groups tabs were text-height only;
- `attachShadow` page error appears on some signed-in contexts — previously identified as estate injected chrome, not Darkhold-owned.

## Decision
Fix only Darkhold-owned SPA code. Improve the shared `SignInGate` into a bordered panel with a 44px primary link — this benefits the nine pages that use the same guard. Give Community tabs 44px height, give the forum filter a 44px labelled input and a real 44px `Search posts` link with a gap. Do not invent forum/group/message data, and do not touch the estate `attachShadow` error.

## Changes
- `SignInGate.svelte`: `.note` becomes a centred bordered panel; the link gets `class="gate-link"`, 44px min-height and accent fill; test asserts the class.
- `Community.svelte`: `.tabs button` `min-height:44px`, inline-flex; `.forum-toolbar` flex with gap, `.forum-filter-input` 44px with `aria-label`, and `<a class="search-link">Search posts</a>` instead of a nested button; CSS for both.
- `Community.test.ts`: assert the filter input is labelled and the search link has the 44px class.

Execution note: the signed-out full-page screenshot shows the sticky header overlapping the top of the gate panel at the captured scroll position; DOM metrics still prove a visible 44px link and no small controls. The gate change is shared by nine pages by design. No credentials or session values were retained.

## Verification (before → after)
- Before: signed-out small desktop/phone `2/1`; signed-in Forums `2/2`; tabs text-height; signed-in groups/messages/blocks `0`.
- After command: `r08_community_audit.py` at both widths, signed out and signed in across four tabs.
- After expected: signed-out small `1/0` (desktop theme select only), signed-in Forums `1/0`, other tabs `1/0`; no overflow; Darkhold-owned errors none (estate `attachShadow` remains separate-owner).
- Row becomes Done in `docs/reviews/page-review-queue.md` in the commit carrying the proof.
