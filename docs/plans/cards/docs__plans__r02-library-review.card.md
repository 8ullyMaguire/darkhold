# docs/plans/r02-library-review.md — plan card

Source: `docs/plans/r02-library-review.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

- `frontend/src/routes/Library.svelte`: wrap `.filter-bar`, `.actions`, `.new-shelf` and `.storage` in `total > 0` conditions; use `<Button variant="primary">Import a work</Button>` in the empty state; restyle `.goal-line a` as muted normal-weight text.
- `frontend/src/App.svelte`: wrap the "Writing as" label in a span, give the link a `title`, and pin the handle span as the only ellipsised flex child (`display:inline`, `min-width:0`, `overflow:hidden`, `text-overflow:ellipsis`).
- Row becomes Done in `docs/reviews/page-review-queue.md` in the same commit whose message names R-02 and whose body carries the before/after line.

## Copy-pasteable code / commands

No fenced command block was recorded in the source document.

## Verification command and expected output

- Tests: `Library.test.ts` empty case asserts no filter bar / storage / duplicate import CTA and one visible `Import a work`; `App.test.ts` asserts the title carries the handle.
Execution note: the first empty-state assertion only checked `textContent`, so a `total >= 0` mutant still passed because `any tag` is an attribute. The assertion now queries the DOM for `input[placeholder="any tag"]`; that mutant fails with `expected <input …> to be null` and the restored code is green.
- After expected: signed-out goal link ≥44px or demoted to non-interactive text; signed-in no filter/storage/duplicate import, one primary `Import a work`, identity label fully visible with ellipsised handle inside its box, no horizontal overflow, no page errors from Darkhold-owned code.
- Targeted tests: `Library.test.ts`, `App.test.ts`, `svelte-check`.

## RED/GREEN evidence

Execution note: the first empty-state assertion only checked `textContent`, so a `total >= 0` mutant still passed because `any tag` is an attribute. The assertion now queries the DOM for `input[placeholder="any tag"]`; that mutant fails with `expected <input …> to be null` and the restored code is green.
