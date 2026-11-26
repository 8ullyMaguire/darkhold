# R-08 — `/community` live DOM review

Evidence: Playwright at 1440×1000 and 390×844, signed out and signed in (temporary staff session created from the existing owner-authorised curator credentials; session file deleted after the row), Forums / Groups / Messages / Blocks tabs. Screenshots: `r08-community-{out,in}-{desktop,phone}-*.png`. DOM JSON: `r08-community-audit.json`.

## GOOD
- One `h1` (`Community`) in all ten cases; no horizontal overflow.
- Signed-in tabs have real `role="tab"` and `aria-selected`, and disabled state before sign-in is correctly removed once a session exists.
- Four existing forums render as cards; empty groups/messages/blocks say so plainly rather than pretending content exists.
- No Darkhold-owned page errors; the intermittent `attachShadow` error is injected estate chrome, previously logged as separate-owner.

## BAD
- Signed-out gate was a 22px inline `Sign in` link floating in an empty beige block, with no visible panel or button.
- Signed-in Forums filter input was 21px tall, placeholder-only, and `Search posts` was a 22px link nested around a button with no gap.
- Tabs were text-height only; Messages/Blocks targets were easy to miss.
- Forum cards carry only a name — no description, count or activity — so a reader cannot choose without opening each one.

## UGLY
- Header envelope icon duplicates the Messages tab and shows an unexplained green dot.
- `Writing as @curatora20261125` is clipped to `Writing as` on desktop.
- Footer `Community` link points at the current page; `Radio` has no description.
- Estate bottom-bar sign-in and chat FAB compete with the page's own action.

## SUGGESTION (separate owner / data)
- Add real forum descriptions, thread counts and last-activity timestamps at the data/API layer; do not fabricate them in the client.
- Fix estate injected chrome (`attachShadow`, envelope dot, bottom bar) and the estate identity clipping.

## Fix in this row
Only Darkhold-owned SPA code: make the shared sign-in gate a bordered panel with a 44px primary link, give Community tabs 44px height, and give the forum filter a labelled 44px input plus a real 44px `Search posts` link.

## After measurement
Appended when the row is closed.

## After measurement (deployed build)
- Vite exit `0`; release build exit `0` in 7m 12s; `/` `200`; `/community` `200` signed out and signed in at both widths. Temporary staff session file was deleted after the audit.
- Small controls before → after: signed-out desktop `2 → 1` (estate theme select), phone `1 → 0`; signed-in Forums `2 → 0` at both widths; Groups/Messages/Blocks `0`. No horizontal overflow in any case.
- The shared `SignInGate` now renders a dashed-border panel with a 44px accent `Sign in` link and the purpose sentence, so the same fix reaches the other pages that use that guard. The forum filter has `aria-label="Filter forums"`, a 44px input and a real 44px `Search posts` link with a gap; tabs are 44px tall.
- Targeted proof: `SignInGate` `4/4`, `Community` `4/4`, `svelte-check` `0 errors and 10 warnings`. Mutant renaming `.gate-link` failed `expected 'gate-broken' to contain 'gate-link'`; restored green.
- The intermittent `attachShadow` page error remains on some signed-in contexts: injected estate chrome, not Darkhold-owned. Forum cards still carry only names because descriptions/counts are not in the API; both are logged as separate-owner/data SUGGESTION.
