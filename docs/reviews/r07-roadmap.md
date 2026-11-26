# R-07 — `/roadmap` live DOM review

Evidence: Playwright at 1440×1000 and 390×844, in-app navigation from `/library`, Board / Arena / Changelog tabs. Screenshots: `r07-roadmap-{desktop,phone}-{board,arena,changelog}.png`. DOM JSON: `r07-roadmap-audit.json`.

## GOOD
- One `h1` (`Roadmap`) and a clear tagline on all six cases; no horizontal overflow and no Darkhold page errors.
- Three named tabs work and keep the URL on `/roadmap`; the Changelog empty state is honest (`No moves yet.`).
- The board empty state says the instance has no cards instead of faking six empty columns.
- Footer and cross-site links are stable across tabs.

## BAD
- Board `Sign in` and `vote in the arena` were 22px inline links; arena `Sign in` was 22px.
- Active tab was a filled rust pill, visually identical to a CTA; inactive tabs were small text.
- Board empty state was left-aligned in a narrow column with a large void; no container or primary action.
- Arena empty state was one small sentence in a void.

## UGLY
- Page title, tab `Arena` and empty copy use three overlapping mental models (Roadmap / Arena / voting).
- Header `Sign in`, estate bottom-bar `Sign in` and empty-state `Sign in` all compete; the header is the loudest but least contextual.
- The `More` nav badge has no count or tooltip.

## SUGGESTION (separate owner / data)
- Suppress the estate bottom-bar sign-in on this route and explain the `More` badge in the estate shell.
- Do not invent feature cards or ballot data to fill the board; it is genuinely empty on this instance.

## Fix in this row
Only Darkhold-owned SPA code: restyle tabs as 44px tabs with an underline active state, centre the board empty state in a bordered panel with a primary `Sign in to suggest` button and a secondary `vote in the arena` link, and give Arena a primary `Sign in to vote` button.

## After measurement
Appended when the row is closed.

## After measurement (deployed build)
- Vite exit `0`; release build exit `0` in 7m 12s; `/` `200`; `/roadmap` `200` at both widths on all three tabs.
- Small controls before → after: desktop board `3 → 1`, arena `2 → 1`, changelog `1 → 1` (remaining item is the estate theme select); phone board `2 → 0`, arena `1 → 0`, changelog `0 → 0`. No horizontal overflow and no Darkhold page errors in any case.
- Tabs are now 44px-tall underline tabs, not a filled CTA pill. The board empty state is a centred dashed-border panel with a primary `Sign in to suggest` button; arena has a primary `Sign in to vote` button; the 22px inline links are gone.
- Targeted proof: `Roadmap` `10/10`, `svelte-check` `0 errors and 10 warnings`. Mutant renaming `.cta-button` to `.cta-broken` failed `expected 'cta-broken' to contain 'cta-button'`; restored green.
- No feature cards or ballot data were invented: the empty board and empty ballot are the live state. Header/estate sign-in duplication and the unexplained `More` badge remain separate-owner SUGGESTION.
