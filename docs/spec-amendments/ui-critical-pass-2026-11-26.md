# UI critical pass — 2026-11-26

## Problem

A full live sweep of 37 signed-out and signed-in routes (148 probes: two themes, two widths) found that the earlier signed-out/Library repairs did not carry through the whole product. The site still presents three visual systems: server-rendered documents (`/`, `/search`, `/works/...`), the Svelte shell, and the nginx-injected Polaris chrome. The same visitor can see two navigations and two auth states on one page. The `archive` palette declares dark tokens but paints light. The landing page's metadata column wraps one word per line. The reader inherits author HTML with duplicate headings and impossible metadata lines.

Evidence: `/home/alvaro/.hermes/profiles/coding/cache/scratch/darkhold-ui-sweep-dark.json`; screenshots in the browser-use workspace named `dark-*.png` and `light-in-roadmap-desktop.png`. Canary moved contrast 0 -> 1 and gutter mutation moved measured padding 192 -> 176, so the probes are live measurements, not vacuous defaults.

## Goals

1. One coherent shell: no duplicate Search, no "Sign in" beside a signed-in account control, and one visual language for nav/actions.
2. The `archive` palette resolves to dark chrome; `after-hours` and `clear-day` remain distinct and legible.
3. The landing page reads as an editorial front door: metadata on one wrapping line, a visible "Browse all" door, no six-identical-author list.
4. The reader stays a no-JavaScript document for reading, but gains a small external enhancement script for automatic position restore, demoted author headings, a sticky progress rail, and a readable metadata block.
5. Standalone controls are at least 44px; heading order no longer skips h1 -> h3; loading and empty states have structure and a real next action.
6. All changes are verified live at desktop and phone widths in both applicable themes.

## Non-goals

- No change to the reader's `--reader-*` scale or to ADR 0033: the Archive is chrome, the Reading Room is prose. A warm-paper reader inside dark chrome is intended; the defect is the duplicated author HTML and missing controls, not the palette difference.
- No rewrite of the scraper/import pipeline or repair of historical imported metadata values. Presentation may label imported metadata honestly; the data fix is a separate row.
- No removal of the injected Polaris bar from all 19 estate sites. It becomes auth-aware and darkhold-tuned with defaults that preserve other sites.
- No new plugin system, no dependency additions beyond what is already installed, and no change to the Rust frontend embed mechanism.
- No full workspace gate mid-turn; only targeted tests, then the gate once at the end.

## Constraints

- The server-rendered reader must remain fully readable with JavaScript disabled. The user chose a tiny progressive enhancement; ADR 0006's "no `<script>`" guarantee is amended to "no inline script; all reading content and controls are in the HTML before the external script runs" (ADR 0034).
- CSP is `script-src 'self'`, so the enhancement must be an external file served by the Rust asset handler, not an inline `<script>` or a nonce.
- The injected chrome lives in a shadow root. Site CSS cannot reach it; custom properties can be inherited. Changes to `estate/ui/src/chrome/topbar.js` must keep its failure mode (no capabilities/nav => empty element) and pass `node --check`.
- Frontend builds use `node node_modules/vite/bin/vite.js build`, not `pnpm build`.
- Thinkcentre release builds must run through `~/.local/bin/estate-build-mode.sh start|end` with `status` OFF afterwards. Do not kill managed taggers.
- Use `CARGO_PROFILE_RELEASE_LTO=off`, `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16`, `CARGO_TARGET_DIR="$HOME/.cargo-target/darkhold"`.

## Acceptance evidence

| id | requirement | command / observation |
| --- | --- | --- |
| UC-1 | `archive` computes a dark body background | Playwright `data-theme=archive` => `--surface-0` and body bg are dark; `after-hours`/`clear-day` still differ |
| UC-2 | no duplicate auth or Search | signed-in DOM has exactly one visible `Sign in` string, zero, and one account control |
| UC-3 | landing metadata does not wrap per word | `.shelf-list .meta` has client width > 400px at 1440 and no single-word lines at 390 |
| UC-4 | author diversity | first six linked starters have at most two distinct handles on the seeded live instance |
| UC-5 | reader headings | exactly one `h1` in document; imported `h1`/`h2` demoted to styled paragraphs |
| UC-6 | reader restore | external `/reader-position.js` served 200 under CSP; `data-restore-permille` present when a stored position exists |
| UC-7 | tap targets | after excluding inline prose links, zero standalone controls under 44px on all swept routes |
| UC-8 | heading order | zero h1 -> h3 skips on the signed-in sweep |
| UC-9 | empty/loading states | `/roadmap` empty state has a "Suggest a feature" action; `/library` empty state has an "Import a work" action; notifications/jobs show skeleton + failure text |
| UC-10 | live proof | screenshots and JSON sweep at both widths and both themes, 0 page errors |
