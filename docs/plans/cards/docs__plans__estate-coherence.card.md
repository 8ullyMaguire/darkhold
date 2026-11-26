# docs/plans/estate-coherence.md — plan card

Source: `docs/plans/estate-coherence.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

work items are `M72-*` in `docs/requirements.csv`.
* **ficnexus has no frontend.** It was reduced on purpose (`docs/EXPORTER-SCOPE.md`: "there are no accounts, no
* **kindred reproduces AO3's visual grammar** on purpose (`web/src/styles/kindred.css` opens by citing
  palettes, `a11y.mjs` (contrast), `docs/census.md`.
* `estate/ui/docs/census.md` — a measured census of **24 primitives** across eight frontends, with a chosen
* `estate/ui/src/chrome/topbar.js` — a framework-free web component **appended to every site by nginx**:
* `estate.toml` + `generated/nav.json` — every site's hostname, ports, unit, tier (`sfw`/`nsfw`), group
`"@polaris/ui": "file:../../../estate/ui/dist/polaris-ui-0.1.2.tgz"` — which resolves only on this machine,
**Layer 1 — distribution.** `@polaris/ui` publishes to the estate's Forgejo npm registry (`estate.toml`
depends on `^0.1.2` / `^0.1.1` through `frontend/.npmrc`'s `@polaris:registry`, and a clone with no `estate/`
`NPM_CONFIG_USERCONFIG`) and never in a tracked file; and `frontend/pnpm-workspace.yaml` still held pnpm's
* **ascend** — `{code,message,details}` frozen in its own `docs/CONTRACTS.md`, codes in lowercase. A frozen

## Copy-pasteable code / commands

No fenced command block was recorded in the source document.

## Verification command and expected output

The gate found two real defects on the way: darkhold's control boundary was 1.8:1 (WCAG 1.4.11 wants 3:1) and its dark
  (measured: an incomplete registration answered `422` with **no body**). Both fixed; a test asserts four failure
Three of the four changed in the *body they send*, which is why every one of them has a test that asserts the
sliced, and absent fields omitted rather than rendered empty. Five render tests assert the order by reading element
* **The probe is the test.** `estate/scripts/meta-probe.mjs` asks every product the same two questions and
   and it is a gate rather than a document.
  checked by a headless render, not by a unit test of the component in isolation.
* Contract rows: one test per service asserting the exact body shape, plus a live probe of all three on this

## RED/GREEN evidence

backwards), and three mutants red: title always a link, byline losing its conjunction, warnings dropped.
  something else owns is how a green board reports the wrong product). `ascend-api.service` runs from
  (`~/kindred-rs-build`) is non-git too — it came up green on the probe without this session deploying it, with
  service is down"). It was run against the live estate before any of this landed and failed on all four —
Every row carries the same shape as the rest of this ledger: a command, a mutant where the row asserts a
* Token/palette rows: `node estate/ui/a11y.mjs` reports contrast for **every** palette; a mutant that sets body
