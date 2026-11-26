# docs/ROADMAP-stale-popular-unrated-search.md — plan card

Source: `docs/ROADMAP-stale-popular-unrated-search.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

`docs/spec-amendments/stale-popular-unrated-search.md` · Implementable plan:
`docs/plans/stale-popular-unrated-search.md`
  celestial or CYOA (`crates/domain/src/tropes.rs`).

## Copy-pasteable code / commands

GET /api/v1/search?q=(trope:system-grind OR tag:celestial OR tag:cyoa)&limit=100

## Verification command and expected output

Two-engine tests, route test, live probe on the deployed binary, ledger and
redefinition of `updated:`, no frontend redesign, no full-workspace gate. See

## RED/GREEN evidence

No RED/GREEN evidence was recorded in the source document.
