# R-06 — `/sign-in` live DOM review

Evidence: Playwright at 1440×1000 and 390×844, in-app navigation from the signed-out header, default and after submitting an empty form (no credentials typed). Screenshots: `r06-signin-{desktop,phone}-{default,error}.png`. DOM JSON: `r06-signin-audit.json`.

## GOOD
- One `h1` (`Sign in`) in all four cases; no horizontal overflow and no Darkhold page errors.
- The error summary uses plain language (`that email address and password do not match an account`) and does not disclose which field was wrong — the anti-enumeration note under the form is honest.
- Labels sit above inputs, both with `autocomplete` set (`email`, `current-password`).
- `Continue with Polaris` is offered as a separate path, and the form has a clear primary submit.

## BAD
- Secondary links `Create an account` and `Forgot your password?` were 25px tall.
- The form column hugged the left edge; the right half of the desktop viewport was empty.
- `Continue with Polaris` was a bordered button whose label was underlined like a link.
- `Forgot your password?` was buried in the action row instead of beside the Password field.
- On `/sign-in` the header and phone nav still offered `Sign in`, so the same action appeared three times on the page (four with the estate bottom bar).

## UGLY
- `Code AUTH_REQUIRED` and a raw request UUID sit in the user-facing error box; the UUID wraps mid-string and is not copyable.
- The header `Sign in` button is the most saturated control and is not the page's action.
- The security note interrupts the gap between Password and the submit button.
- Estate bottom-bar `Sign in`, the white-on-cream Polaris wordmark and the scales icon are separate-owner chrome issues.

## SUGGESTION (separate owner / shared component)
- Put technical error codes and request IDs behind a `Technical details` disclosure; do not change the shared `ErrorSummary` from this one page.
- Add a copy-to-clipboard affordance to the request ID.
- Fix the estate bar's duplicate sign-in and low-contrast wordmark in the estate theme.

## Fix in this row
Only Darkhold-owned SPA code: centre the form, give secondary/recovery links 44px targets, remove the false underline from the Polaris control, move Forgot beside Password, and suppress the header/nav sign-in button on this route.

## After measurement
Appended when the row is closed.

## After measurement (deployed build)
- Vite exit `0`; release build exit `0` in 6m 54s; `/` `200`. `/sign-in` `200` at both widths, default and error state.
- Small controls before → after: desktop `3 → 1` (remaining item is the estate theme select), phone `2 → 0`; no horizontal overflow and no Darkhold page errors in any case.
- Form column is centred at the desktop midpoint; `Continue with Polaris` keeps its bordered button shape but the label has no underline; `Forgot your password?` sits right-aligned under Password; only the form's `Sign in` button remains in the Darkhold header/nav (the bottom-bar entry is estate chrome).
- Targeted proof: `SignIn` `4/4`, `App` `14/14`, `svelte-check` `0 errors and 10 warnings`. Mutant renaming `.forgot` to `.note` failed the recovery-link assertion; restored green.
- `AUTH_REQUIRED`, the raw request UUID and the estate bottom-bar duplicate remain SUGGESTION: the shared `ErrorSummary` and estate theme are not owned by this row.
