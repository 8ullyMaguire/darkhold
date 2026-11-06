# 0007 — The design system is already owned; the identity is not

Status: accepted, 2026-11-06. Decides M71-02; sets the vocabulary for M71-03…M71-07.

## Problem

"I don't like how the site turned out" is a statement about identity, and the obvious explanation — that Darkhold
borrows another project's design system — is only a third true. Checked against the tree:

- `frontend/src/styles/tokens.css` is 322 lines of Darkhold's own tokens ("The Reading Room"), with
  `docs/design/THEME.md` as its written specification, three polish presets, and a separate `--reader-*` scale.
- `frontend/src/lib/components` holds **80** local components, including `EmptyState`, `ForumVoteBar`,
  `KarmaBadge`, `DnfPanel`, `ContinueReadingBanner`, `JobProgress`, `Dialog`, `Drawer`, `Combobox`.
- `@polaris/ui` — the estate package, a local tarball from `~/estate/ui` — supplies six things: `Button`, `Card`,
  `Empty`, `Field`, `Tag`, and a topbar script. Darkhold uses `Button` in 28 files, `Field` in 16, `Empty` in 2,
  `Tag` in 2, `Card` in **0**.

So the interface is not an estate template with content poured in; it is a mostly-owned component library sitting on
owned tokens, sharing button height and field chrome with fifteen sibling sites. What it lacks is not a colour
scheme. It lacks:

1. a **component language for the catalogue** — there is no dense row, no provenance mark, no reason line, no facet
   group, no console row; every list is a card grid because cards are what a generic dashboard has;
2. a **chosen visual direction** — tokens exist, a direction was never decided, so each surface solved its own page
   and the result is consistent only in spacing;
3. **structure that is ours** — buttons and fields read as every other polarisocial site because their shape comes
   from a shared artefact, which is precisely the layer a reader notices when they use two sites in one day.

The earlier review in the vault said "43 files import `@polaris/ui`, that is why it looks like the other sites". The
count is right and the inference was too strong: those imports are `Button` and `Field`. Corrected here, because a
decision built on the wrong cause builds the wrong thing.

## Decision

**Own the semantic layer (already true), decide one direction, and build the structural components that carry it.
Share only what is invisible.**

1. **`tokens.css` stays the single source of truth** and is extended, not replaced: add the roles the catalogue needs
   (provenance, rating, status, density steps) and keep `--reader-*` independent of the site scale.
2. **Three named directions are written** in `docs/design/DIRECTIONS.md`, each justified by the product's register,
   each applied to the same three surfaces (a browse list, a work page, the console overview) before a winner is
   chosen. Naming them is the point: a direction that is not named cannot be rejected.
3. **The direction chosen** is *The Archive* for site chrome — dense index-card rows, monospace metadata, ink on
   paper, provenance as a stamp rather than an icon — with *The Reading Room* kept for the reader surface (prose
   wants calm, catalogues want density) and one signature accent borrowed from *The Forbidden Section* for
   provenance, rating and console status marks only.
4. **Structural components become Darkhold's**: `DenseRow`, `ProvenanceMark`, `ReasonLine`, `StatTile`, `FacetGroup`,
   `ConsoleRow`, `ReaderBar`, `LockedChip`. Each reads the tokens; none imports `@polaris/ui`.
5. **Primitives are migrated last and only where shape carries identity**: `Button` and `Field` get Darkhold-owned
   equivalents (same focus ring, same target sizes, same a11y contract) so that the two sites stop sharing a button.
   `Empty`, `Tag` and `Card` are dropped or folded into the new components — `Card` is already unused.
6. **What stays shared is what a reader cannot see**: the `a11y.mjs` contrast verifier and the semantic token
   contract. Sharing a checker is not sharing a look.

## Alternatives and why they lost

- **Adopt the external proposal's palette as written.** Rejected on measurement, not taste: `--text-muted #5c5955`
   on `--surface-base #0f0f12` is 2.75:1 while the document claims 3.7 and uses it for metadata; white on the
   violet accent `#b39ddb` is 2.40:1, which is every primary button in the dark preset; white on the
   high-contrast accent `#ffff00` is 1.07:1, which is invisible. Its four functional presets survive as a
   requirement; its identity does not survive its own gate.
- **Fork `~/estate/ui` into this repo.** Duplicates a sixteen-site artefact and guarantees drift; the package is
  small enough that "fork" buys nothing that "own the semantic layer" does not already buy.
- **Re-theme only (keep every component, change values).** Cheapest, and it answers the complaint halfway: the
  colours change, the card grid stays, and the site still reads as a dashboard in a different jumper. The catalogue
  needs a row, not a recoloured card.
- **Replace the frontend wholesale.** That is M71-01's territory and the answer there is per-page migration, not a
  second application.

## Consequences

- Two visual languages coexist deliberately: dense for the catalogue and console, calm for prose. The boundary is
  the reader surface, which already has its own scale, so no page has to choose per element.
- Every new component is a component the estate does not have to maintain, and every remaining `@polaris/ui` import
  is a TODO with a reason (`Button` ×28, `Field` ×16 are the migration list; `Empty` ×2 and `Tag` ×2 fold into the
  new set outright).
- The contrast verifier becomes a gate rather than a document: every declared pair is measured, including on-accent
  text, in every preset — the failure the external proposal shipped three times.

## Evidence

- `frontend/src/styles/tokens.css` (322 lines), `frontend/src/styles/reader.css` (384), `frontend/src/app.css`
  (245), `docs/design/THEME.md` (133) — the owned layer that exists today.
- `grep -rl "@polaris/ui" frontend/src` → 43 files; by symbol: `Button` 28, `Field` 16, `Empty` 2, `Tag` 2,
  `Card` 0.
- `~/estate/ui/src/lib` → `Button.svelte`, `Card.svelte`, `Empty.svelte`, `Field.svelte`, `Tag.svelte`,
  `index.ts`; plus `tokens/primitives.json`, `tokens/semantic.json`, `a11y.mjs`, `verify.mjs`.
