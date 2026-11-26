# R-02 — `/library` live DOM review

Evidence: Playwright at 1440×1000 and 390×844, signed out and signed in, `r02_library_audit.py` and
`r02_writing_as_probe.py`, 2026-11-26. Screenshots: `r02-library-signed-out-1440.png`,
`r02-library-signed-out-390.png`, `r02-library-signed-in-1440.png`, `r02-library-signed-in-390.png`.
DOM JSON: `r02-library-audit.json`.

## GOOD
- One `h1` (`Library`) in all four cases; `scrollWidth` equals viewport (`1440/1440`, `390/390`); zero overflow.
- The signed-out card has a clean eyebrow → serif heading → copy → two-button ladder, and the active Library nav item is unambiguous.
- The signed-in empty state pattern (dashed card, `Nothing here`, one sentence, one action) is the strongest part of the signed-in page.
- Serif/rust/cream identity is coherent and bookish; small-caps `SHELVES` / `STORAGE` labels add texture.

## BAD
- Signed out: `small=1` at both widths — `Set a reading goal` was 22px tall and visually louder than the secondary CTA.
- Signed out: three Sign in affordances in one viewport (header button, card primary, estate-bar pill).
- Signed in: `small=6` at both widths; the filter bar and storage telemetry appeared before the empty state at 0 works.
- Signed in: two labels for one import action (`Import something` and `Import a work`) in two accent colours.
- Signed in desktop: `attachShadow` page error from the injected estate chrome.
- Header identity: `Writing as` was a flex item and `strong` was `display:block`; at 1440 the handle box ran from x=1060 to x=1200 inside a 176px link ending at x=1183, so it clipped mid-handle.

## UGLY
- Storage copy leaked implementation language: "stored bytes, each blob counted once" and "Frees 0 bytes by deleting the 0 stored copies…" — developer telemetry in a reader's library.
- `Sort` sat alone on a second filter row; native date and select controls clashed with the custom button system.
- Header showed `iting as@curatora202611` in the screenshot — a broken-looking identity at the exact place a writer checks who they are posting as.

## SUGGESTION (estate owner)
- Bottom bar's `Sign in` pill is a third peer CTA; its Polaris wordmark is low-contrast; `All sites` and icon-only buttons are small on phone; the `attachShadow` error comes from the injected bar. None is Darkhold document code.
- Footer's `Radio` item has no description and sits close to the fixed bar on phone; that footer is estate chrome.

## Fix in this row
Darkhold-owned SPA only: demote the signed-out goal link; hide filters, maintenance actions, new-shelf form and storage at 0 works; one primary `Import a work`; and a single-line identity status with a full-handle `title`.
Before: signed-out `small=1/1`; signed-in `small=6/6`, filters + storage visible at 0 works, identity handle clipped.
After measurement is appended to `docs/reviews/page-review-queue.md` when the row is closed.

## After measurement (deployed build)
- `/library` `200`; Vite build exit `0`; release build exit `0` in 7m 33s.
- Signed out: `small=0` at both widths; `h1=Library`, `scroll=1440/1440` and `390/390`, no Darkhold page errors.
- Signed in: `small=0` at both widths; 0-work page now reads `Library | Set a reading goal | SHELVES | All works 0 | Nothing here | ... | Import a work` — no filter bar, no storage telemetry, no `Import something` duplicate.
- Identity: `scrollWidth=176`, `clientWidth=176`, `max-width=176.175px`; label stays intact and the handle no longer paints outside the box.
- An intermittent `attachShadow` error remains; it comes from the injected estate bar, not Darkhold document code, and is logged as SUGGESTION above.
- Targeted proof: `Library.test.ts` `10 passed`, `App.test.ts` `14 passed`, `ReadingGoalRing.test.ts` `5 passed`, `svelte-check` `0 errors and 10 warnings`. Mutant `total > 0` → `total >= 0` on the filter bar failed `expected <input …> to be null`, then was restored green.
