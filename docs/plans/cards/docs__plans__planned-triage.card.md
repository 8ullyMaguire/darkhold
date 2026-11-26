# docs/plans/planned-triage.md — plan card

Source: `docs/plans/planned-triage.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

| M45-58 — erasure reaches `supply_routing` | `erasure::UnlinkedColumn { table: "supply_routing", column: "account_id" }` rewrites the link to `erased:<row id>`; the row survives (§58.3) and the account leaves (§51). | `crates/db/tests/supply_routing_erasure.rs` 4/4 and `erasure_covers_fk_free_accounts` 3/3, both green 2026-11-06 |
| M71-02 — three directions, one chosen in an ADR | The deliverable is the writing plus the decision. | `docs/design/DIRECTIONS.md`; `docs/adr/0007-the-design-system-is-owned-the-identity-is-not.md` |
Enumerated in `docs/plans/roadmap-remaining.md`; this file is the reasoning, that one is the sequence.

## Copy-pasteable code / commands

No fenced command block was recorded in the source document.

## Verification command and expected output

1. The four "missing one thing" rows — each is a contained change with an obvious test.

## RED/GREEN evidence

| M45-58 — erasure reaches `supply_routing` | `erasure::UnlinkedColumn { table: "supply_routing", column: "account_id" }` rewrites the link to `erased:<row id>`; the row survives (§58.3) and the account leaves (§51). | `crates/db/tests/supply_routing_erasure.rs` 4/4 and `erasure_covers_fk_free_accounts` 3/3, both green 2026-11-06 |
