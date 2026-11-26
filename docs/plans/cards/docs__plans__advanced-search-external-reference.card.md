# docs/plans/advanced-search-external-reference.md — plan card

Source: `docs/plans/advanced-search-external-reference.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

> **Status: pasted reference, not yet reconciled with `docs/spec.md`.** Saved verbatim as
4. **Vespa/OpenSearch/Elasticsearch** vs the spec's Postgres + SQLite two-dialect architecture (`scripts/postgres-journey.sh`, dual-migration `migrations/sqlite/` + `migrations/postgres/`). Adding a third search engine is a large architectural commitment the spec has so far avoided.
The `ExistsCharacter`/`ExistsRelationship` AST nodes in §15.3 have **no implementation and no schema**: there is no migration defining character or relationship tables, and `QueryAst` in `crates/domain/src/query.rs` has only `Text | Phrase | Fielded | Comparison | And | Or | Not`. Characters and ships exist only as `taxonomy_nodes` rows with `kind = 'character'` / `'ship'`, which supports a flat name match and nothing else — no prominence, no attributes, no participants.

## Copy-pasteable code / commands

fandom:"Star Wars" 
AND ship:(Obi-Wan/Anakin OR "Obi-Wan & Anakin")
AND trope:"slow burn"
AND NOT warning:"Major Character Death"
AND words:20k..100k
AND status:complete
AND updated:<2y
AND kudos_pct:>top20%

## Verification command and expected output

- **Eval:** logged queries, zero-result rates, and click and read-through metrics, with an offline relevance test set built from bookmarks and recs as ground truth.
If you tell me your expected scale, whether you want reader-applied tags, and your stance on ML-inferred metadata, I can propose a concrete tag schema and index mapping.
- Expected scale (determines whether SQLite FTS5 is acceptable or a real engine is needed).

## RED/GREEN evidence

No RED/GREEN evidence was recorded in the source document.
