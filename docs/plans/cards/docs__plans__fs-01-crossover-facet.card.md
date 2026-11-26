# docs/plans/fs-01-crossover-facet.md — plan card

Source: `docs/plans/fs-01-crossover-facet.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

Work from `/home/alvaro/code/rust/darkhold`. Spec: §15.7 and §15.15 (`docs/spec.md:2780`, `:6045`), roadmap
## Step 1 — parser/AST (`crates/domain/src/query.rs`)
## Step 2 — SQL renderer (`crates/domain/src/query_sql.rs`)
## Step 3 — dual-engine test (`crates/db/tests/crossover_facet.rs`)
`stale_popular_unrated_search.rs`, and `search_works_ast_scoped`. Fixture:
Also add a pure parser test in `crates/domain/src/query.rs` tests: `crossover:maybe` is a `QueryError`, and
## Step 5 — UI control (`frontend/src/routes/Search.svelte`)

## Copy-pasteable code / commands

QueryField::Crossover => {
    let (operator, label) = match value {
        "exclude" => ("<=", "crossover:exclude"),
        "only" => (">", "crossover:only"),
        _ => return Err(QueryError::new("crossover requires exclude or only", 0)),
    };
    let _ = label;
    Ok(SqlFragment::new(format!(
        "(SELECT COUNT(DISTINCT wt.node_id) FROM work_tags wt \
         WHERE wt.work_id = works.id AND EXISTS \
         (SELECT 1 FROM canonical_entities ce WHERE ce.id = wt.node_id AND ce.kind = 'fandom')) {operator} 1"
    )))
}

let excluded = search(db, "crossover:exclude").await;
assert_eq!(excluded, vec![SINGLE.to_owned()], "exclude must drop the crossover");
let only = search(db, "crossover:only").await;
assert_eq!(only, vec![XC.to_owned()], "only must keep exactly the crossover");

cd /home/alvaro/code/rust/darkhold
cargo test -p darkhold-domain crossover
cargo test -p darkhold-db --test crossover_facet                       # SQLite
DARKHOLD_TEST_PG_URL=postgres://darkhold:darkhold@localhost:55433/darkhold \
  cargo test -p darkhold-db --test crossover_facet                     # PostgreSQL

## Verification command and expected output

## Step 3 — dual-engine test (`crates/db/tests/crossover_facet.rs`)
Also add a pure parser test in `crates/domain/src/query.rs` tests: `crossover:maybe` is a `QueryError`, and
cargo test -p darkhold-domain crossover
cargo test -p darkhold-db --test crossover_facet                       # SQLite
  cargo test -p darkhold-db --test crossover_facet                     # PostgreSQL
Expected: parser tests pass; dual-engine test passes once on each engine.
Mutant guard: temporarily change `"only" => (">", ...)` to `("<=", ...)`, rerun the SQLite test, confirm it fails,
Add a DOM test asserting the two buttons emit `crossover:exclude` and `crossover:only` as last `searchWorks` calls.
Expected: a 200 JSON body with `facets` and `hits` (no 422). Then add a `crossover` preset button to the SPA Search

## RED/GREEN evidence

mutant must fail.
Mutant guard: temporarily change `"only" => (">", ...)` to `("<=", ...)`, rerun the SQLite test, confirm it fails,
restore, and rerun to green. Record the exact failure line in the handoff.
