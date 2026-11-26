# docs/plans/r06-signin-review.md — plan card

Source: `docs/plans/r06-signin-review.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

- `SignIn.svelte`: `.page { margin-inline:auto; }`, `.polaris-button { text-decoration:none; }`, action links `display:inline-flex; align-items:center; min-height:44px`, Forgot link moved after the Password field into `.forgot` with the same 44px rule.
- `App.svelte`: both anonymous `Sign in` buttons render only when `route.id !== 'sign-in'`.
- Row becomes Done in `docs/reviews/page-review-queue.md` in the commit carrying the proof.

## Copy-pasteable code / commands

No fenced command block was recorded in the source document.

## Verification command and expected output

- `SignIn.test.ts`: assert the Forgot link is present, the Polaris control has no underline class, and links have the action/forgot class; targeted Vitest + `svelte-check`.
- After expected: secondary links 44px; phone small `0/0`; desktop small only the estate theme select; two `Sign in` controls total (form + estate bar); no overflow, no page errors.

## RED/GREEN evidence

No RED/GREEN evidence was recorded in the source document.
