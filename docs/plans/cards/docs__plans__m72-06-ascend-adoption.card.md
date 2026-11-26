# docs/plans/m72-06-ascend-adoption.md — plan card

Source: `docs/plans/m72-06-ascend-adoption.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

`--accent`. ascend's `web/src/app.css` defines **none** of them:
grep -c -- '--p-space-2' src/app.css   # 0
grep -c -- '--text-1'    src/app.css   # 0
(`gate.sh design`) holds ascend at two widths.
- Not the moderation queue. `routes/moderation/+page.svelte:370` is the closest existing blurb but it is an
palette in the package. `web/src/app.css`, after the Tailwind import:
@import '@polaris/ui/tokens.css';
@import '@polaris/ui/palettes/ascend.css';
and in `web/src/app.html`, `<html lang="en" data-palette="ascend">`.
grep -o -- '--p-space-2:[^;]*' .svelte-kit/output/client/_app/immutable/assets/*.css | head -1   # --p-space-2: 8px;
grep -c 'data-palette="ascend"' build/../.svelte-kit/output/server/app.html 2>/dev/null || true
source, not a stale copy: `node -e "console.log(require.resolve('@polaris/ui/package.json'))"`.

## Copy-pasteable code / commands

grep -c -- '--p-space-2' src/app.css   # 0
grep -c -- '--text-1'    src/app.css   # 0

dist/tokens.css            --p-space-1..16, --p-radius-*, --p-dur-*, --p-ease-*, and the semantic names
                           (--text-1, --border-1, ...) at :root
dist/palettes/ascend.css   [data-palette="ascend"] — dark surfaces at hue 280, --text-1 at 96% lightness
dist/palettes/archive.css  [data-palette="archive"]  — darkhold's paper

@import 'tailwindcss';
@import '@polaris/ui/tokens.css';
@import '@polaris/ui/palettes/ascend.css';

## Verification command and expected output

`/fics` lists each fic through `WorkBlurb`, with the family's spacing and type scale, and the public gate sweep
(`gate.sh design`) holds ascend at two widths.
  authenticated admin surface, and the gate cannot see it. `/fics` is public and measured today (94 text nodes).
Verify — the palette must reach the built CSS, and the attribute must reach the served document:
**4. Prove it in the artifact, not in a test.** The lesson from darkhold's half — `WorkCard` rendered the blurb, four
# expected: at least one non-zero count
node ~/code/estate/ui/design-gate.mjs --name ascend
# expected: 4 page render(s), 0 failure(s) — and, new, blurbs present:

## RED/GREEN evidence

tests passed, and nothing imported the component:
**6. Mutant.** Remove `--p-space-2` from the mapping block and the sweep must redden on
`thin-page`/`contrast` — if it stays green, the blurb is not what the page is rendering and the adoption is inert.
