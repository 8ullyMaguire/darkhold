# docs/plans/advanced-search-phase1-substrate.md — plan card

Source: `docs/plans/advanced-search-phase1-substrate.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

| §15.3 defines `ExistsCharacter` / `ExistsRelationship` | `grep docs/spec.md` | **true** — `docs/spec.md:2365` |
| neither is in the built AST | `crates/domain/src/query.rs:451` | **true** — `QueryAst` has only `Text \| Phrase \| Fielded \| Comparison \| And \| Or \| Not` |
| no character/relationship schema exists | `grep migrations/` | **true** — only `taxonomy_nodes` rows with `kind='character'`/`'ship'` |
| journey 12 is an acceptance criterion | `docs/spec.md` §25.1 item 12 | **true** — "Search bound character attribute → exclude ship → filter by mood" |
| `work_tags` has no prominence | `migrations/sqlite/0011_taxonomy.sql` | **true** — `work_tags(work_id, node_id, weight, added_at)` |
| §15.17 already governs `review_status` | `migrations/sqlite/0082_taxonomy_review_status.sql` | **true** — columns exist; must not be reinvented |
`migrations/sqlite/` + `migrations/postgres/`, with parity tests that tax every
   `scripts/probe-character-substrate-pg.sh` applies it to a real PostgreSQL 15 and
   `crates/db/src/work_characters.rs`.

## Copy-pasteable code / commands

-- Character attributes, §15.1: prominence, roles, attributes
work_characters(work_id, character_node_id, prominence, is_pov)

-- A ship is a participant SET. Identity = the sorted set, so "A/B" and "B/A"
-- are one node and no writer has to pick an order.
ship_participants(ship_node_id, character_node_id)

-- The relationship TYPE belongs to the work, not the ship (§15.2).
work_relationships(id, work_id, ship_node_id, rel_type, prominence, dynamics)

-- Tag prominence + source, per the as-built doc's §2.
ALTER work_tags ADD prominence / source / confidence

title: author: fandom: character: relationship: tag: mood:

## Verification command and expected output

behind a `VectorIndex` trait, and the reference's open question ("expected scale")
must test the *same* `work_characters` row. A naive three-`EXISTS` compilation —
correlated `AND`s on `work_characters` itself, and the test suite includes a
1. **Migration 0103, both dialects + parity gate.** Done. The parity test compares

## RED/GREEN evidence

No RED/GREEN evidence was recorded in the source document.
