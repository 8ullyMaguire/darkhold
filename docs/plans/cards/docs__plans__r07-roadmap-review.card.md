# docs/plans/r07-roadmap-review.md — plan card

Source: `docs/plans/r07-roadmap-review.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

- `Roadmap.svelte`: tab CSS (`min-height:44px`, underline active), `.board-empty` centred panel, `.empty-cta`/`.arena-signin .button` with 44px links and accent fill, arena copy updated.
- Row becomes Done in `docs/reviews/page-review-queue.md` in the commit carrying the proof.

## Copy-pasteable code / commands

No fenced command block was recorded in the source document.

## Verification command and expected output

- `Roadmap.test.ts`: extend empty-state test to assert the two CTA links with classes and the tab active class; targeted Vitest + `svelte-check`.
Execution note: `svelte-check` passed with 0 errors; the arena test needed its text assertion retargeted from the old inline sentence to the new `Sign in to vote` button. The footer links and estate bar are deliberately out of this row.
- After expected: board small desktop `1` (estate select), phone `0`; arena/changelog `1/0`; no overflow, no page errors; tabs 44px; primary CTA visible.

## RED/GREEN evidence

Execution note: `svelte-check` passed with 0 errors; the arena test needed its text assertion retargeted from the old inline sentence to the new `Sign in to vote` button. The footer links and estate bar are deliberately out of this row.
