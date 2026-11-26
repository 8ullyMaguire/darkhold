# docs/plans/ui-critical-pass-2026-11-26.md — plan card

Source: `docs/plans/ui-critical-pass-2026-11-26.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

- `docs/spec-amendments/ui-critical-pass-2026-11-26.md`
- `docs/adr/0034-reader-progressive-enhancement.md`
git add docs/spec-amendments/ui-critical-pass-2026-11-26.md docs/adr/0034-reader-progressive-enhancement.md
File: `frontend/src/styles/tokens.css`.
@import '@polaris/ui/palettes/reading-room.css';
@import '@polaris/ui/palettes/archive.css';
@import '@polaris/ui/palettes/after-hours.css';
@import '@polaris/ui/palettes/clear-day.css';
File: `/home/alvaro/code/estate/ui/src/chrome/topbar.js`.
node --check /home/alvaro/code/estate/ui/src/chrome/topbar.js && echo TOPBAR_PARSE_OK
ssh thinkcentre 'cd /personal/documents/code/estate && sudo bash scripts/install-chrome.sh darkhold.polarisocial.xyz'
File: `crates/app/src/routes/document.rs`. Change the body open to:

## Copy-pasteable code / commands

cd /home/alvaro/code/rust/darkhold
git add docs/spec-amendments/ui-critical-pass-2026-11-26.md docs/adr/0034-reader-progressive-enhancement.md
git commit -m "docs: specify the UI critical pass and reader enhancement"
git status --short

@import '@polaris/ui/palettes/reading-room.css';
@import '@polaris/ui/palettes/archive.css';
@import '@polaris/ui/palettes/after-hours.css';
@import '@polaris/ui/palettes/clear-day.css';

  /* The injected estate chrome lives in a shadow root; inherited custom properties are the only styling contract it has. */
  --polaris-bar-bg: var(--surface-0);
  --polaris-bar-fg: var(--text-primary);
  --polaris-bar-dim: var(--text-secondary);
  --polaris-bar-line: var(--border-subtle, rgba(128,128,128,.3));

## Verification command and expected output

# UI critical pass — implementation plan (2026-11-26)
Work from `/home/alvaro/code/rust/darkhold` unless a step says otherwise. Do not start before the spec and ADR are committed. Every step ends with its exact verification command. Only the last step runs the repository gate.
- `docs/spec-amendments/ui-critical-pass-2026-11-26.md`
git add docs/spec-amendments/ui-critical-pass-2026-11-26.md docs/adr/0034-reader-progressive-enhancement.md
git commit -m "docs: specify the UI critical pass and reader enhancement"
Expected: empty status.
Verify:
Expected: `BUILD1_OK`, archive rule appears after reading-room root rule.
Verify:
Verify:
cd frontend && npx vitest run src/App.test.ts 2>&1 | tail -5
Expected: `SHELL_RUST_OK` and passing App tests.
Verify (targeted test file only):
cargo test -p darkhold-app --test landing_page 2>&1 | tail -8
Expected: all tests pass, including a new assertion that `.meta` contains spans and `Browse all` links to `/search`.
Update `crates/app/tests/reader_surface.rs` (or the nearest reader document test) to assert: no inline `<script>`, one external `src="/reader-position.js"`, one `h1`, zero `<h2>` inside `.reader-surface__body`, and a `data-restore-permille` attribute when a position is passed.
Verify:
cargo test -p darkhold-app --test reader_surface 2>&1 | tail -8
Expected: all pass.
Verify with a signed-in Playwright query using the existing sweep harness:

## RED/GREEN evidence

Update `crates/app/tests/reader_surface.rs` (or the nearest reader document test) to assert: no inline `<script>`, one external `src="/reader-position.js"`, one `h1`, zero `<h2>` inside `.reader-surface__body`, and a `data-restore-permille` attribute when a position is passed.
