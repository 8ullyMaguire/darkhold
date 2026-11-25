# Plan — Library and shell UX fix, 2026-11-25

Spec: `docs/spec-amendments/library-shell-ux-2026-11-25.md`. All paths are
inside `frontend/`. Change only Svelte/CSS and the tests named here. Do not
touch the Rust backend. Work on thinkcentre; build with vite's binary directly.

## Step 1 — nav model and its tests

File `src/App.svelte`:
- `NAV_GROUPS`: rename `Read` to `Explore more`, `Forum` to `Community`.
  Add `{ href: '/write', label: 'Write' }` as the first item of `Create` and
  delete the stale comment saying `/write` is deliberately not listed.
- `PRIMARY`: replace the four entries with
  `Discover`, `Search`, `Library`, `Shelf` (`href: '/library/history'`).
  `Write` is now only in the Create group.
- The phone drawer already renders `navGroups`. The desktop nav was the trap: it
  also rendered every group as an inline `NavMenu`, so six menus plus four links
  overflowed (live `nav.scrollWidth=812`, `clientWidth=522`). Replace the group
  loop in `nav.desktop` with one `<NavMenu label="More" items={moreNavItems}>`,
  where `moreNavItems = $derived(navGroups.flatMap((group) => group.items))`.
  Every old href stays reachable and the row becomes four links plus one trigger.
  Both desktop and phone read `primaryNav`, so they stay in agreement.
- `src/lib/theme.ts`: change `system: 'Match system'` to
  `system: 'System default'`.

File `src/App.test.ts`:
- Line ~117 list becomes `['Discover', 'Search', 'Library', 'Shelf']`.
- Line ~125 menu list becomes
  `['Explore more', 'Create', 'Shelf', 'Community', 'More']`.
- The duplicate-label assertion stays. Assert `triggers` equals `['More']` and
  `nav.desktop > *` has exactly 5 children; open More for the queue and
  Surprise Me checks instead of the old group names.
- Line ~270: find the trigger named `Explore more`, not `Read`.

Verify: `npx vitest run src/App.test.ts` — all tests pass.

## Step 2 — signed-out Library card

File `src/routes/Library.svelte`:
- Move `<ReadingGoalRing />` from line 407 into the `{:else}` branch, first.
- Replace lines 409–413 with a `<section class="signed-out">` containing a
  card: eyebrow `Your shelf, synced`, `h2` `Sign in to see your library`, one
  explanatory paragraph, a `.signed-out-actions` row with `Button variant=
  "primary"` → `/sign-in` and `Button variant="secondary"` → `/discover`, and
  a final `.goal-line` with the existing `Set a reading goal` link worded as
  a tertiary action after sign-in.
- Add scoped CSS: `.signed-out { min-height: 24rem; display: grid; align-items:
  center; }`, `.signed-out-card { max-width: 46rem; border: 1px solid
  var(--color-border); border-radius: var(--radius-lg); padding:
  var(--space-6); background: var(--color-surface-raised); }`,
  `.signed-out-actions { display: flex; flex-wrap: wrap; gap: var(--space-3);
  margin-top: var(--space-5); }`, `.goal-line { margin-top: var(--space-5);
  color: var(--color-muted); }`. Body copy uses `var(--text-base)`.
- `navigate` is already imported; do not add a second router.

Verify: `npx vitest run src/routes/Library.test.ts` — all tests pass.

## Step 3 — header controls, hit targets, install notice

File `src/App.svelte`:
- Mobile nav (`nav.mobile`): use `primaryNav`, not the unfiltered `PRIMARY`,
  and add an account door — `Sign in` signed-out, `Account` (opens the More
  drawer) signed-in. Tighten `.mobile { gap: var(--space-1); padding-right:
  var(--space-3); }` and set `.site-header button { white-space: nowrap; }`
  so six controls fit at 390px with a right gutter. Both were proved by live
  CSS injection (nav 390/390, button 73px, right edge 371px). This came from the phone screenshot, not the desktop audit.
- Signed-out controls: replace the plain `Sign in` anchor with
  `<Button variant="primary" size="sm" onclick={() => navigate('/sign-in')}>
  Sign in</Button>`; keep Register as a link. Add a visible
  `<label class="control-label" for="theme-select-anon">Appearance</label>`
  before the select.
- Add `let pwaDismissed = $state(false);` and in `onMount`, before
  `registerPwa`, read `localStorage.getItem('darkhold:pwa-notice-dismissed')`.
- Gate the notice with `{#if (installAction || updateAction) && !pwaDismissed}`.
  Replace the native `<button>` elements with design-system `Button`s:
  `Install for offline reading` and `Reload to update`. Add a dismiss
  `Button variant="quiet" size="sm"` with `aria-label="Dismiss notice"` that
  sets `pwaDismissed = true` and writes the localStorage key.
- CSS (global `src/app.css`, so child components' controls are reached):
  `.site-header a, .site-header button, .site-header select, .site-footer a,
  .skip-link, .pwa-notice button { display:inline-flex; align-items:center;
  min-height: 2.75rem; }` plus `.footer-links a { min-width: 2.75rem;
  justify-content: center; }`. A second live pass found short labels (Shelf
  40px, More 41px) and added `min-width` to `.site-header a` and `.site-header
  button` too. Both passes proved zero sub-44 controls by CSS injection in the
  live page before rebuilding. The first draft's `.desktop nav a` selector was
  wrong — the class is on `nav` itself. `.control-label` stays scoped in
  App.svelte.
  Keep the existing wrap rules; reducing the row from nine to five links is
  what makes the taller targets fit.

Verify: `npx vitest run src/App.test.ts` — all tests pass.

## Step 4 — footer and body type

File `src/App.svelte`:
- Replace the `book-divider`, long paragraph and sibling heading block with a
  two-column `.footer-grid`: brand/tagline, a local utility nav
  (`/docs`, `/roadmap`, `/community`) and `<FamilyNav self="darkhold" />`.
- CSS: delete `.book-divider` rules, set `.footer-grid { display:flex;
  flex-wrap:wrap; justify-content:space-between; gap: var(--space-5); }`,
  `.footer-links { display:flex; flex-wrap:wrap; gap: var(--space-4); }`,
  `main { font-size: var(--text-base); }`, reduce footer padding to
  `var(--space-4) 0 var(--space-5)`.
- FamilyNav retains the Kindred/Radio links; they must wrap, not clip.

Verify: `npx vitest run src/App.test.ts src/routes/Library.test.ts`.

## Step 5 — gates, once, at the end

- `node node_modules/vite/bin/vite.js build` — must print `✓ built in`.
- `npx svelte-check --threshold error` — no new errors from touched files.
- Live: rebuild frontend, restart darkhold only if the served asset hash
  changed, then re-open `/library` at 1440×1000 and 390×844 and measure:
  `scrollWidth <= clientWidth`, zero standalone controls under 44px, signed-out
  card with two buttons, five nav links, account control present.
- Report measured numbers, not adjectives.

## Preservation gate

Every original href in `NAV_GROUPS`, `PRIMARY`, `ACCOUNT_ITEMS` and the drawer
must still render somewhere. If a destination disappears, the plan is wrong and
must be fixed before shipping.
