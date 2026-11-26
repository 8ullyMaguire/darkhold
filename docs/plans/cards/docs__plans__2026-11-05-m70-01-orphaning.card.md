# docs/plans/2026-11-05-m70-01-orphaning.md — plan card

Source: `docs/plans/2026-11-05-m70-01-orphaning.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

    migrations/sqlite/0003_works.sql:41
    migrations/sqlite/0001_identity.sql:74
1. **ADR** `docs/adr/0004-orphaned-works-and-detached-ownership.md` — rejected alternative, erasure consequence,
3. **`crates/db/src/orphan.rs`**: `orphan_work(db, work_id) -> Result<FrozenLabel, Refusal>`, one transaction: read

## Copy-pasteable code / commands

No fenced command block was recorded in the source document.

## Verification command and expected output

   owner". The scaffolding test asserts the work survives, its title is still searchable, the author line ends in

## RED/GREEN evidence

No RED/GREEN evidence was recorded in the source document.
