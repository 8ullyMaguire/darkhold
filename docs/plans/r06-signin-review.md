# R-06 — sign-in page: plan and acceptance

## Problem
Live Playwright via in-app navigation at 1440×1000 and 390×844, default and after submitting an empty form:
- secondary links `Create an account` and `Forgot your password?` are 25px tall;
- desktop form column hugs the left edge, leaving the right half empty;
- `Continue with Polaris` is a bordered button whose label is underlined like a link;
- `Forgot your password?` sits in the action row, away from the Password field;
- on the sign-in route the header and phone nav still offer a `Sign in` button, so the page shows the same action three times.

## Decision
Fix only Darkhold-owned SPA code. Centre the form column, give action links 44px targets, remove the false underlining from the Polaris button, move `Forgot your password?` directly under the Password field, and suppress the header/nav `Sign in` button while already on `/sign-in`. Leave the estate bottom-bar duplicate, `AUTH_REQUIRED` code and request UUID as separate-owner/technical-detail SUGGESTION; changing the shared ErrorSummary for one page would alter every error surface.

## Changes
- `SignIn.svelte`: `.page { margin-inline:auto; }`, `.polaris-button { text-decoration:none; }`, action links `display:inline-flex; align-items:center; min-height:44px`, Forgot link moved after the Password field into `.forgot` with the same 44px rule.
- `App.svelte`: both anonymous `Sign in` buttons render only when `route.id !== 'sign-in'`.
- `SignIn.test.ts`: assert the Forgot link is present, the Polaris control has no underline class, and links have the action/forgot class; targeted Vitest + `svelte-check`.

Execution note: a static screenshot cannot show a 44px invisible hit area; the DOM measurement (small=0 on phone, 1 on desktop for the estate select) is the evidence. The two Darkhold header/nav Sign in buttons are suppressed only on `route.id === 'sign-in'`; the estate bottom bar is unchanged.

## Verification (before → after)
- Before: default/error small desktop `3/3`, phone `2/2`; two secondary links at 25px; three `Sign in` controls on the page.
- After command: `r06_signin_audit.py` at both widths, default and error state.
- After expected: secondary links 44px; phone small `0/0`; desktop small only the estate theme select; two `Sign in` controls total (form + estate bar); no overflow, no page errors.
- Row becomes Done in `docs/reviews/page-review-queue.md` in the commit carrying the proof.
