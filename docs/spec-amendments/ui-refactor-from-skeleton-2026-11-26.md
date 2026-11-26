# UI refactor from the greenfield skeleton — 2026-11-26

**Status:** accepted, not built. This amendment records a refactor direction, not shipped code.
**Authority:** `docs/requirements.csv`, `docs/spec.md`, ADR 0033, ADR 0034, and the 2026-11-26 build comparison.
**Evidence:** `/home/alvaro/darkhold-build-comparison-2026-11-26.md` (score table, screenshots, probe JSON in `/home/alvaro/darkhold-comparison-evidence/`), `docs/spec-amendments/ui-critical-pass-2026-11-26.md`, `docs/adr/0035-port-skeleton-patterns-not-the-stack.md`.

## Problem

The 2026-11-26 side-by-side comparison scored a greenfield skeleton **8.15/10** and the live
Darkhold build **5.20/10** on a fixed rubric (visual craft 20%, first-run comprehension/IA 20%,
decision support 25%, interaction/recovery 15%, responsive 10%, accessibility/robustness 10%).
The gap is not visual taste alone: the prototype wins where the product makes a decision —
availability, evidence, reasons, recovery — while the live build wins on real content and backend
robustness.

Measured on live build `0.1.0+24e5b2cd` (origin tunnel `127.0.0.1:8081`; the UI-critical-pass
commits `452b4087` and `ee830b5c` are ancestors, so this is not a stale deploy):

- server-rendered documents (`/`, `/search`, `/works/…`) carry no **site** navigation; SPA
  surfaces (`/arena`, `/library`) carry the app nav plus nginx-injected `<polaris-chrome>`.
  The injected shadow-DOM bar is a **Polaris network switcher** (verified on the public host:
  `polaris-chrome.shadowRoot` lists 12 estate sites), not a Darkhold wayfinding system.
- at 390px adjacent SPA nav links sit 4px apart; home result links are 652×28.
- the reader control strip computes `opacity: 0.14` at `scrollY = 0` on a 390px viewport — an intended
  scroll-recede effect firing at the wrong end; hover/focus restores it, touch users do not;
- the same work is presented as `by devwriter · 5,367 words` on `/` and
  `By: MarcusBlack` / `Words: 6220` in the reader.
- both builds had 0 document overflow at 1440 and 390 and 0 contrast failures after the probe
  was taught to convert `oklch()`; the prototype had a skip link, the live home did not.

The prototype has its own defects: `/find?q=mystery` returns **0 works** although home suggests
"quiet mysteries", the "Find works" label wraps, `/work/:id` has no active nav state, the
wordmark is a 129×34 target, and its publishing/permissions/exports are simulations.

## Decision

Port **patterns, not the stack**: keep Rust/Axum, Svelte, the two-renderer split (ADR 0006), and
the Archive-chrome / Reading-Room-reader registers (ADR 0033). Bring over the pieces that made
the prototype better at decision support and wayfinding; repair the live hard failures first.
No rewrite, no React/Vite migration, no mock persistence in production.

## Goals

1. One product-level wayfinding system on every surface, with an active state, without adding a
   second nav beside the injected Polaris bar (UC-1 of the UI critical pass remains the rule).
2. Availability and evidence stated in words wherever a work appears, including server-rendered
   lists: `Readable here`, `Reference only · text not held`, `unmeasured`.
3. A work dossier with fixed facts, provenance, `Choose this if` / `Avoid this if`, a biggest-unknown
   line, and an export action, on both the SPA and server-rendered work surfaces where applicable.
4. Constrained search whose query and filters live in the URL, with a named zero-result recovery
   and a closed sort vocabulary; no silent fallback.
5. Honest copy for absent, degraded, conflict and module-unavailable states.
6. Every change verified with a targeted test and a live DOM measurement at 1440 and 390.

## Non-goals

- No new component library, no `@polaris/ui` replacement, no change to ADR 0033 or 0034.
- No reader type-scale or palette rewrite. A warm-paper reader inside dark chrome is intended.
- No change to the scraper/import pipeline or to historical imported metadata values; the
  author/word-count conflict is a **presentation gate plus a separate data row**, not a silent
  data mutation.
- No explicit reader-side ranking feedback here. That is already parked as `XW-1` in
  `docs/plans/suki-crosswalk-intake-2026-11-26.md` and stays gated on the experiment loop being
  enabled (live had 0 snapshots and 0 decisions).
- No full workspace gate mid-turn; targeted tests only, then the existing gate at the end.
- No deploy in this docs pass.

## Constraints

- Server-rendered documents stay documents: no inline script, no client-only content needed to
  read or navigate (ADR 0006, ADR 0034).
- Injected chrome lives in a shadow root; site CSS cannot reach it. Custom properties inherited
  from `:root` are the only styling contract; keep the "no capabilities/nav => empty element"
  failure mode of `estate/ui/src/chrome/topbar.js` intact.
- Frontend builds use `node node_modules/vite/bin/vite.js build`, not `pnpm build`.
- Thinkcentre release builds go through `~/.local/bin/estate-build-mode.sh start|end` with
  `status` OFF afterwards; use `CARGO_PROFILE_RELEASE_LTO=off`,
  `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16`, `CARGO_TARGET_DIR="$HOME/.cargo-target/darkhold"`.
- Do not kill managed taggers. Do not merge the three visual systems into one component tree.

## Acceptance evidence

| id | requirement | command / observation |
| --- | --- | --- |
| UIR-0 | reader strip is readable at scroll 0; conflicting work metadata is gated or corrected | computed `opacity` of `.reader-surface__controls` at `scrollY=0` is `>= 0.6`; `/` and reader agree on author and word count for the same work |
| UIR-1 | site nav exists on server-rendered documents and has one active state | public `/` and `/search` each have one Darkhold-destination nav in addition to `<polaris-chrome>`; no duplicate Sign in; SPA nav still `scrollWidth <= clientWidth` at 390 |
| UIR-2 | every work row states availability | `/`, `/search` result rows and SPA work rows print `Readable here` or `Reference only · text not held`; unknown prints `unmeasured`, never `0` |
| UIR-3 | work dossier anatomy present | work surface shows provenance facts, `Choose this if`, `Avoid this if`, biggest unknown, export action; `cargo test -p darkhold-app --test landing_page` green |
| UIR-4 | search constraints and recovery | URL carries query and filters; unknown sort names the word; zero results name the narrowest constraint and offer removal; `cargo test -p darkhold-app --test search_page` green |
| UIR-5 | honest state copy | absent/degraded/conflict/module-off states use the copy deck below; no "locked" or "coming soon" placeholder |
| UIR-6 | live proof | DOM probe at 1440 and 390 on `/`, `/search`, one work, `/arena`, `/library`; 0 page overflow, 0 non-prose controls under 44px, 0 contrast failures including inherited opacity |

## Copy to carry over

- Absent: "No editorial position is published. An instance statement would appear here when published."
- Degraded: "Text is not held. The source address is unmeasured."
- Empty search: "No works match these constraints. Remove the last constraint."
- `REVISION_CONFLICT`: "Your write was based on an older revision. Choose how to merge."
- `MODULE_UNAVAILABLE`: "This operation is unavailable because its module is not enabled."
- `NOT_ENOUGH_EVIDENCE`: "There is not enough evidence for a verdict."

## Refusals

- **Rewrite the frontend in React.** Rejected: the comparison scores patterns, not stack; the
  backend and two-renderer topology are working and ADR 0006/0033 remain decisive.
- **Copy the prototype's route names and mock data.** Rejected: those names were invented to avoid
  poisoning the design brief; the product already owns its routes and vocabulary.
- **Ship XW-1 as part of this refactor.** Rejected: the experiment loop is off, so the UI would be inert.
- **Treat the injected Polaris bar as a defect to delete.** Rejected: it serves all 12 estate sites;
  scope stays with auth-aware, darkhold-tuned defaults as recorded in the UI critical pass.
- **Call the split-chrome finding "no nav anywhere".** Rejected: public server-rendered pages do
  inject a network switcher; what is missing is a Darkhold-destination nav, not every link.

## Ordering

UIR-0 first (a visible-at-a-glance defect and a truth defect outrank polish), then UIR-1 (wayfinding
is the largest comprehension win), then UIR-2/UIR-3/UIR-4 (decision support), then UIR-5/UIR-6.
Detailed code steps are in `docs/plans/ui-refactor-from-skeleton-2026-11-26.md`.
