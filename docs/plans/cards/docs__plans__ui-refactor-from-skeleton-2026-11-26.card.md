# docs/plans/ui-refactor-from-skeleton-2026-11-26.md — plan card

Source: `docs/plans/ui-refactor-from-skeleton-2026-11-26.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

**Source:** `docs/spec-amendments/ui-refactor-from-skeleton-2026-11-26.md`
**Decision:** `docs/adr/0035-port-skeleton-patterns-not-the-stack.md`
**File:** `frontend/src/styles/reader.css` (lines 439–449, `@keyframes reader-controls-recede`).
**Files:** `crates/app/src/routes/landing_page.rs` (Starter projection, lines 194–221 and 415–420),
`crates/app/src/routes/work_page.rs` (public contributor/word projection, lines 145–212),
`crates/app/tests/landing_page.rs`.
use the same public-contributor and readable-unit word sum as `work_page.rs` (functions
pattern already exist at `work_page.rs:145-146`). If the two projections are legitimately different
**Files:** `crates/app/src/routes/landing_page.rs` (`render`, lines 254–332),
`crates/app/src/routes/search_page.rs`, `crates/app/src/routes/work_page.rs`,
`crates/app/src/App.svelte` (`NAV_GROUPS`, `primaryNav`, header at line 395),
`estate/ui/src/chrome/topbar.js` (read-only constraint).

## Copy-pasteable code / commands

@keyframes reader-controls-recede {
  from { opacity: 1; transform: none; }
  to { opacity: 0.62; transform: translateY(-0.2rem); }
}

cd /home/alvaro/code/rust/darkhold/frontend
node node_modules/vite/bin/vite.js build

cd /home/alvaro/code/rust/darkhold
cargo test -p darkhold-app --test landing_page

## Verification command and expected output

pass**, and the verification commands were **not executed here**. Run each command at its step and
**Verify (browser, 390×844):** navigate to a reader URL, set `scrollY = 0`, read
`getComputedStyle(document.querySelector('.reader-surface__controls')).opacity`. Expected: `>= 0.6`.
Then scroll to 400 and re-read: expected still `>= 0.6`. Focus the strip: expected `1`.
**Verify (build):**
Expected: `✓ built in …` and no TypeScript error. This command is the documented frontend build;
`Words: 6220` for the same work id. Do not silently rewrite imported metadata. Add a test that
**Verify:**
cargo test -p darkhold-app --test landing_page
Expected: the new disagreement test passes; quote its first line in the card. Mutation: change
**Verify (repo):**
cargo test -p darkhold-app --test route_inventory
Expected: no new route; 6 passed (as recorded in `docs/plans/roadmap-remaining.md` for this target).
**Verify (public host, after deploy):**
Expected: `1` (the estate bar is still present). Then in the browser at 1440 and 390, assert one
and exactly one visible account/sign-in control. At 390 assert app nav `scrollWidth <= clientWidth`
`frontend/src/lib/components/WorkCard.test.ts`, `frontend/src/lib/components/WorkRow.test.ts`.
**Verify:**
cargo test -p darkhold-app --test landing_page
cargo test -p darkhold-app --test search_page

## RED/GREEN evidence

paste the real output into the card before marking it done. Do not invent a green result.
the shelf fixture author by one character, re-run, require `FAILED`, restore.
Expected: no new route; 6 passed (as recorded in `docs/plans/roadmap-remaining.md` for this target).
- UIR-0a/0b fixed with mutation evidence, not just a green test.
