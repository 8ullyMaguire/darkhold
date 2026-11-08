# The family design gate (M72-09)

`estate/ui/design-gate.mjs` reviews the family's surfaces and fails when they drift. It is a gate, not a document:
it renders, it measures, it exits non-zero.

```
node design-gate.mjs --name darkhold          # one product, every palette, 1280 and 390
node design-gate.mjs --all                    # every product in TARGETS
node design-gate.mjs --self-test              # the gate must be able to fail, and able to pass
node design-gate.mjs --mutant contrast|focus  # falsify a live sweep
```

Exit 0 = every check passed on every rendered page. Exit 1 = at least one failure, named with the element and the
measured ratio. Exit 2 = the gate could not run at all (no browser, unknown target).

## What it checks, and why each one exists

| rule | what it measures | the defect it exists for |
|---|---|---|
| `contrast` | every visible text node's ink against the paper it is actually painted on, AA (4.5:1, or 3:1 for large text) | 2026-10-27: `/roadmap`'s tab strip and `/search`'s "Add" were white on cream (1.11) in the light palettes while `after-hours` was perfect |
| `focus-ring` | Tab to the first focusable element, then require a real outline (≥2px) or a real shadow | a focus style that exists in one component and not the next |
| `reduced-motion` | any element still transitioning/animating >100 ms under `prefers-reduced-motion: reduce` | motion budgets get agreed and then ignored |
| `heading-order` | first heading is `h1`, no level skips | DOM order that reads as a wall to a screen reader |
| `type-scale` | more than 12 distinct computed font sizes on one page | the token vocabulary is eight steps; thirteen sizes is a product writing its own |
| `overflow` | `scrollWidth` vs viewport at 390 px | one wide table makes the whole site scroll sideways on a phone |
| `thin-page` | fewer than 20 text nodes | a gate pointed at a URL that renders the router's "No such page" reports a clean sweep of nothing |
| `palette-not-applied` | the document's `data-theme` equals the palette under test | three palettes that are silently the same page measured three times |
| `colour-unreadable` | every text node's colour resolved | a regex over `getComputedStyle().color` dropped 99 of 100 nodes and the sweep stayed green |

## Three false greens this gate found in itself

Recorded because each one is the same mistake in a new costume — measuring something adjacent to the claim.

1. **The palette switch was a no-op.** `?theme=` is ignored by darkhold and `data-theme` stayed unset, so a
   three-palette sweep measured the default three times. The gate now sets the stored preference before the app
   boots and fails `palette-not-applied` if the document does not carry the palette it asked for.
2. **The focus-ring predicate accepted anything.** Outline *colour* defaults to `currentColor`, so
   `outlineColor !== 'rgb(0, 0, 0)'` was true on a bare link with no focus style at all — the check passed a page it
   was written to fail. It now requires a real outline width or a real shadow.
3. **The contrast probe could not read the ink.** Parsing `getComputedStyle().color` with `/rgba?\(…\)/` missed
   every modern colour syntax and silently skipped those nodes. Colours are now resolved by painting them on a
   canvas, and anything unresolvable is a `colour-unreadable` failure rather than a skip.

Each was found by asking what the number would look like if the check were broken, then building the mutant that
proves it is not: `--mutant contrast` repaints every element's ink in its own paper, `--mutant focus` strips every
ring, and `--self-test` plants the 2026-10-27 defect on a real `<button>` and a link with no focus style — and, in
the other direction, a correct page that must produce no failures at all.

## Reports

Each run writes `design-gate-<timestamp>.json` here: every page rendered, its text-node count, the distinct font
sizes, the heading sequence, and every failure with the element, the sample text, the ratio and the paper's origin.

## What it deliberately does not do

- **No screenshots as evidence.** A pixel diff fails on a font hint and passes on white-on-cream.
- **No palette-JSON audit.** `estate/ui/a11y.mjs` already checks the palette definitions; this checks what the page
  actually paints, because a component can override a token and leave the palette file correct.
- **No verdict on kindred's AO3 grammar.** kindred reproduces the Archive's DOM on purpose; the gate measures
  legibility and mechanics there, not resemblance to the other products.
