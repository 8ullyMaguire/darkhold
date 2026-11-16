# 0033 — The Archive is the chrome; the Reading Room is the reader

**Date:** 2026-11-16
**Status:** Accepted
**Context:** `docs/design/DIRECTIONS.md`, `docs/design/THEME.md`, `docs/spec/21-frontend-and-design.md`, ADR 0007, and the redesign plan at `/home/alvaro/.hermes/plans/2026-11-16_155746-darkhold-from-scratch-redesign.md`.

## Problem

Three design documents give three answers to the same question. `DIRECTIONS.md` and ADR 0007 choose **The Archive** (dense rows, monospace metadata, provenance stamps) for site chrome and keep **The Reading Room** for prose. `THEME.md` calls Reading Room authoritative without scoping it to the reader. Spec §21 asks for a Novellunar-style dark cover-card grid. A model without vision cannot resolve that contradiction from pixels; it guesses, and each guess makes the site a different product.

## Decision

The two registers are fixed and are not a matter of taste:

* **Site chrome is The Archive.** Browse, search, library, rails, settings and the operator console use dense rows, hairline dividers, one monospace metadata scale, and provenance/status as words. The `archive` palette in `@polaris/ui` is the dark default for chrome.
* **The reader is The Reading Room.** Paper/White/Sepia/Dark, Literata, wide measure, no badges, no points, no suggestion popups. A dark interface with a warm-paper reading page is valid and is the intended combination.
* **The Forbidden Section contributes stamps only** — provenance, rating and status marks, and the console's critical red. It is never the language of a whole page.
* The Novellunar dark cover-card grid is **not** the design of this site. Its useful properties (one nav row, one sidebar, one ranked list, a measured dark scale) are expressed through the Archive row contract instead of copied as a theme.

## Alternatives rejected

**Novellunar dark card grid whole-site.** At 141,317 catalogue rows a card grid shows three rows per screen and has no vocabulary for provenance. It also makes long reading a dark-surface problem.

**Reading Room everywhere.** It was the incumbent and it is correct for prose, but it has no density and no way to say "this row is only referenced from another archive" without inventing a coloured pill.

**Forbidden Section everywhere.** The strongest identity of the three and the worst place to read: it turns settings into a game HUD and ages badly.

## Consequences

Every new surface must state which register it belongs to. `--reader-*` tokens stay independent of site tokens. Colour is never the only signal: provenance and status are printed words with data attributes. Visual properties are enforced by the token/contrast gate, the Playwright DOM-measurement suite and the no-JS reader suite in the redesign plan, not by eyeballing screenshots.

## Verification

```bash
grep -n 'Decision of record' docs/design/DIRECTIONS.md
grep -n 'reader surface only' docs/design/THEME.md
grep -n 'ADR 0033' docs/spec/21-frontend-and-design.md
```

Each prints one matching line.
