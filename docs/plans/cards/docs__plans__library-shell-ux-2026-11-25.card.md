# docs/plans/library-shell-ux-2026-11-25.md — plan card

Source: `docs/plans/library-shell-ux-2026-11-25.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

Spec: `docs/spec-amendments/library-shell-ux-2026-11-25.md`. All paths are
inside `frontend/`. Change only Svelte/CSS and the tests named here. Do not
File `src/App.svelte`:
- `src/lib/theme.ts`: change `system: 'Match system'` to
File `src/App.test.ts`:
Verify: `npx vitest run src/App.test.ts` — all tests pass.
File `src/routes/Library.svelte`:
Verify: `npx vitest run src/routes/Library.test.ts` — all tests pass.
File `src/App.svelte`:
- CSS (global `src/app.css`, so child components' controls are reached):
  App.svelte.
Verify: `npx vitest run src/App.test.ts` — all tests pass.

## Copy-pasteable code / commands

No fenced command block was recorded in the source document.

## Verification command and expected output

File `src/App.test.ts`:
- The duplicate-label assertion stays. Assert `triggers` equals `['More']` and
Verify: `npx vitest run src/App.test.ts` — all tests pass.
Verify: `npx vitest run src/routes/Library.test.ts` — all tests pass.
- Gate the notice with `{#if (installAction || updateAction) && !pwaDismissed}`.
  justify-content: center; }`. A second live pass found short labels (Shelf
Verify: `npx vitest run src/App.test.ts` — all tests pass.
Verify: `npx vitest run src/App.test.ts src/routes/Library.test.ts`.
## Preservation gate

## RED/GREEN evidence

No RED/GREEN evidence was recorded in the source document.
