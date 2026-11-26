# docs/plans/advanced-search-as-built.md — plan card

Source: `docs/plans/advanced-search-as-built.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

> PostgreSQL keeps real CHECKs. See `migrations/sqlite/0103_character_relationships.sql`
> `crates/db/src/spoilers.rs` behind `routes/spoilers.rs` and pinned by five tests in
> `milestone_34_spoilers.rs`. Re-keying would mean a table rebuild with a live route
- **Planner:** `AST → Plan { predicate, lexical, semantic, boosts, sort, facet_requests }`. This refactor of `query_sql*.rs` is the central code change. Each engine implements `compile(plan)`.

## Copy-pasteable code / commands

 text / chips / NL ──► parser ──► AST (versioned) ──► planner ──► Plan
                          ▲            │                           │
                  pretty-printer ◄─────┘            ┌──────────────┼───────────────┐
                                                    ▼              ▼               ▼
                                             SQL predicate   lexical (FTS)   vector (ANN/exact)
                                          (visibility+filters   + snippets    + similar_to / sem:
                                           + fields, ONE source of truth)
                                                    └──────────────┼───────────────┘
                                                                   ▼
                              fuse (RRF) → score features → rerank (MMR) → explain
                                                                   ▼
                       Response { results, facets, chips, explanations, relaxations }

-- existing taxonomy_nodes, extended
ALTER taxonomy_nodes ADD status TEXT NOT NULL DEFAULT 'active'; -- active|pending|merged|deprecated
ALTER taxonomy_nodes ADD merged_into INTEGER NULL;
-- kind widened: fandom, character, ship, trope, genre, mood, setting, format,
-- pov, tense, warning, meta, freeform

-- Fandom scoping (many-to-many: crossovers, shared universes)
taxonomy_node_scope(node_id, fandom_node_id, PRIMARY KEY(node_id, fandom_node_id))

-- Graph edges (DAG for parent/implies; cycle check on write)
taxonomy_edges(src_id, dst_id, rel, created_by, PRIMARY KEY(src_id,dst_id,rel))
  -- rel: parent | implies | related | adjacent_fandom

-- Materialized closure so expansion is a join, not a recursive CTE
taxonomy_closure(ancestor_id, descendant_id, rel, depth,
                 PRIMARY KEY(ancestor_id, descendant_id, rel))

ALTER work_tags ADD prominence TEXT NOT NULL DEFAULT 'secondary'; -- primary|secondary|background
ALTER work_tags ADD source     TEXT NOT NULL DEFAULT 'author';    -- author|reader|inferred|curator
ALTER work_tags ADD confidence REAL NOT NULL DEFAULT 1.0;
ALTER work_tags ADD status     TEXT NOT NULL DEFAULT 'applied';   -- applied|suggested|rejected
-- backfill prominence from existing `weight` with a documented threshold

work_tag_votes(work_id, node_id, voter_pseud_id, vote)   -- reader layer, aggregated into confidence

## Verification command and expected output

- **Pretty-printer:** add a canonical `ast → text` printer. Chips, URLs, saved views, and recipes all depend on it. Test `parse(print(ast)) == ast` with property tests and golden files.
- **Facets** are computed from the same compiled predicate as `WITH cand AS (...)`, then aggregated by node kind. They are **disjunctive**: each facet group's counts omit that group's own filter, so users see what including or excluding would do. Counts are capped or approximate above a threshold (shown as "~"), cached by `(query hash, viewer-filter hash, epoch)`, and subject to a minimum-cell-size rule for anonymous collections. A regression test asserts that facet counts never exceed result-set size.
- **ANN with filters** is the main source of subtle bugs (short pages and recall loss under selective filters). Test with adversarial selectivity.

## RED/GREEN evidence

No RED/GREEN evidence was recorded in the source document.
