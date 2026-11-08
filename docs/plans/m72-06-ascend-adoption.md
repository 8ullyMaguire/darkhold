# M72-06, ascend half — the plan

Status: not started. This is the last adoption in the estate batch; darkhold's is live (20 `article.blurb` elements on
`/search` after a real query), kindred's is refused with reasons (its blurbs are `format!` strings emitting AO3's DOM).

## The problem, measured rather than assumed

`WorkBlurb` styles itself with the family's token names — `--p-space-*`, `--p-step-*`, `--text-1..3`, `--border-1`,
`--accent`. ascend's `web/src/app.css` defines **none** of them:

```
grep -c -- '--p-space-2' src/app.css   # 0
grep -c -- '--text-1'    src/app.css   # 0
```

It carries Tailwind v4 `@theme` variables instead (`--color-rarity-common`, …). Importing the component without the
tokens renders a blurb whose spacing collapses to zero and whose ink falls back to `inherit` — a component that
"adopted" while looking wrong is worse than no adoption, because the next reader concludes the family style is poor.

## Goal

`/fics` lists each fic through `WorkBlurb`, with the family's spacing and type scale, and the public gate sweep
(`gate.sh design`) holds ascend at two widths.

## Non-goals

- Not a rename: ascend's object stays a *fic* (a pointer to a story elsewhere plus the deck built from it). The URL
  link is already done — `/works/{id}` → 302 → `/fics/{id}`, measured through the edge.
- Not a restyle: ascend keeps its dark neutral palette and its rarity colours. The family supplies spacing, type
  scale and focus treatment; ascend supplies its paper.
- Not the moderation queue. `routes/moderation/+page.svelte:370` is the closest existing blurb but it is an
  authenticated admin surface, and the gate cannot see it. `/fics` is public and measured today (94 text nodes).

## Steps

**1. Import the family tokens and ascend's own palette.** The estate already ships both, measured:

```
dist/tokens.css            --p-space-1..16, --p-radius-*, --p-dur-*, --p-ease-*, and the semantic names
                           (--text-1, --border-1, ...) at :root
dist/palettes/ascend.css   [data-palette="ascend"] — dark surfaces at hue 280, --text-1 at 96% lightness
dist/palettes/archive.css  [data-palette="archive"]  — darkhold's paper
```

So there is no hand mapping to invent and no risk of painting ascend in another product's paper: ascend has its own
palette in the package. `web/src/app.css`, after the Tailwind import:

```css
@import 'tailwindcss';
@import '@polaris/ui/tokens.css';
@import '@polaris/ui/palettes/ascend.css';
```

and in `web/src/app.html`, `<html lang="en" data-palette="ascend">`.

Verify — the palette must reach the built CSS, and the attribute must reach the served document:

```bash
cd web && pnpm install --frozen-lockfile && pnpm run build
grep -o -- '--p-space-2:[^;]*' .svelte-kit/output/client/_app/immutable/assets/*.css | head -1   # --p-space-2: 8px;
grep -c 'data-palette="ascend"' build/../.svelte-kit/output/server/app.html 2>/dev/null || true
```

If the palette import lands but the page looks unchanged, the attribute is missing — `[data-palette="ascend"]` is a
selector, so the tokens exist and nothing uses them.

**2. Install the package.** `pnpm add @polaris/ui@^0.5.4` in `web/`, then confirm the resolved path is the published
source, not a stale copy: `node -e "console.log(require.resolve('@polaris/ui/package.json'))"`.

**3. Rewrite the `/fics` pack row.** In `web/src/routes/fics/+page.svelte`, replace the inline `<li>` title/status
markup (lines ~157–172) with:

```svelte
<li>
  <WorkBlurb
    work={{
      title: fic.title,
      authors: [fic.author],
      meta: [fic.status === 'completed' ? 'complete' : fic.status, fic.word_count ? `${Math.round(fic.word_count / 1000)}k words` : ''].filter(Boolean),
      href: fic.url,
    }}
    size="inline"
  />
</li>
```

`href` is the fic's external URL, not `/fics/{id}`: on ascend the fic *is* the link out. Add
`import { WorkBlurb } from '@polaris/ui';` to the script block.

**4. Prove it in the artifact, not in a test.** The lesson from darkhold's half — `WorkCard` rendered the blurb, four
tests passed, and nothing imported the component:

```bash
pnpm run build && grep -c 'blurb svelte-' .svelte-kit/output/client/_app/immutable/assets/*.js
# expected: at least one non-zero count
```

**5. Live, at both widths.** Deploy: copy the route file and the stylesheet to `~/code/ascend/app/web`, `pnpm install
--frozen-lockfile`, `pnpm run build`, `systemctl --user restart ascend-web.service`, then

```bash
node ~/code/estate/ui/design-gate.mjs --name ascend
# expected: 4 page render(s), 0 failure(s) — and, new, blurbs present:
curl -s https://ascend.polarisocial.xyz/fics | grep -c 'class="blurb'
```

**6. Mutant.** Remove `--p-space-2` from the mapping block and the sweep must redden on
`thin-page`/`contrast` — if it stays green, the blurb is not what the page is rendering and the adoption is inert.

## What would make this plan wrong

- If `@polaris/ui/tokens.css` turns out to define whole palettes (not just spacing/type), importing it wholesale would
  repaint ascend in darkhold's paper. Then step 1 imports only the scale variables, copied explicitly.
- If Tailwind v4's `@theme` refuses a plain `:root` block after it, the mapping moves into `@theme` — the check in
  step 1 is what catches that, and it is cheap to run before any component work.
