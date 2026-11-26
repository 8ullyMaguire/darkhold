# docs/plans/r08-community-review.md — plan card

Source: `docs/plans/r08-community-review.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

- `SignInGate.svelte`: `.note` becomes a centred bordered panel; the link gets `class="gate-link"`, 44px min-height and accent fill; test asserts the class.
- `Community.svelte`: `.tabs button` `min-height:44px`, inline-flex; `.forum-toolbar` flex with gap, `.forum-filter-input` 44px with `aria-label`, and `<a class="search-link">Search posts</a>` instead of a nested button; CSS for both.
- Row becomes Done in `docs/reviews/page-review-queue.md` in the commit carrying the proof.

## Copy-pasteable code / commands

No fenced command block was recorded in the source document.

## Verification command and expected output

- signed-out gate was a 22px inline `Sign in` link in otherwise empty space, with no panel or primary action;
- `SignInGate.svelte`: `.note` becomes a centred bordered panel; the link gets `class="gate-link"`, 44px min-height and accent fill; test asserts the class.
- `Community.test.ts`: assert the filter input is labelled and the search link has the 44px class.
Execution note: the signed-out full-page screenshot shows the sticky header overlapping the top of the gate panel at the captured scroll position; DOM metrics still prove a visible 44px link and no small controls. The gate change is shared by nine pages by design. No credentials or session values were retained.
- After expected: signed-out small `1/0` (desktop theme select only), signed-in Forums `1/0`, other tabs `1/0`; no overflow; Darkhold-owned errors none (estate `attachShadow` remains separate-owner).

## RED/GREEN evidence

No RED/GREEN evidence was recorded in the source document.
