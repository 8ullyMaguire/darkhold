# R-02 — `/library` review: plan and acceptance

## Problem
Live Playwright at 1440×1000 and 390×844, signed out and signed in, found:
- signed-out: `small=1/1` (`Set a reading goal` 22px), three Sign in affordances, tertiary goal link louder than the secondary CTA.
- signed-in empty: `small=6/6`, one page error at desktop (`attachShadow` called twice), full filter panel and storage telemetry shown before the `Nothing here` card, two labels for one import action (`Import something` / `Import a work`), and a header identity line whose `strong` is `display:block` inside a 176px flex box, so it clips mid-handle.

## Decision
Fix only Darkhold-owned SPA code in this row. The estate bar's Polaris/All-sites/Sign in duplication and its shadow-root error are estate-chrome defects, logged as SUGGESTION with the estate owner. Keep the signed-out card (it is sound); demote its goal link with CSS. For the empty signed-in library, hide the filter bar, maintenance actions, new-shelf form and storage block at `total === 0`, and make the empty state's import action the single primary button. Make the header identity a single ellipsised line with a `title` carrying the full handle.

## Changes
- `frontend/src/routes/Library.svelte`: wrap `.filter-bar`, `.actions`, `.new-shelf` and `.storage` in `total > 0` conditions; use `<Button variant="primary">Import a work</Button>` in the empty state; restyle `.goal-line a` as muted normal-weight text.
- `frontend/src/App.svelte`: wrap the "Writing as" label in a span, give the link a `title`, and pin the handle span as the only ellipsised flex child (`display:inline`, `min-width:0`, `overflow:hidden`, `text-overflow:ellipsis`).
- Tests: `Library.test.ts` empty case asserts no filter bar / storage / duplicate import CTA and one visible `Import a work`; `App.test.ts` asserts the title carries the handle.

Execution note: the first empty-state assertion only checked `textContent`, so a `total >= 0` mutant still passed because `any tag` is an attribute. The assertion now queries the DOM for `input[placeholder="any tag"]`; that mutant fails with `expected <input …> to be null` and the restored code is green.

## Verification (before → after)
- Before: signed-out `small=1/1`; signed-in `small=6/6`, identity `strong` x=1060→1200 outside the 176px box, filters and storage visible at 0 works.
- After command: `r02_library_audit.py` plus `r02_writing_as_probe.py` against the deployed site at both widths and both auth states.
- After expected: signed-out goal link ≥44px or demoted to non-interactive text; signed-in no filter/storage/duplicate import, one primary `Import a work`, identity label fully visible with ellipsised handle inside its box, no horizontal overflow, no page errors from Darkhold-owned code.
- Targeted tests: `Library.test.ts`, `App.test.ts`, `svelte-check`.
- Row becomes Done in `docs/reviews/page-review-queue.md` in the same commit whose message names R-02 and whose body carries the before/after line.
