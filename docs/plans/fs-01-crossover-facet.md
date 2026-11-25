# FS-01 — crossover include/exclude facet

Work from `/home/alvaro/code/rust/darkhold`. Spec: §15.7 and §15.15 (`docs/spec.md:2780`, `:6045`), roadmap
`FS-01`. Acceptance: dual-engine `crossover:exclude` drops the crossover, `crossover:only` keeps it; URL round trip;
mutant must fail.

## Decision

A crossover is a work linked to **more than one distinct fandom taxonomy node**. There is no boolean column to trust
and no migration: the fact is derivable from `work_tags -> canonical_entities(kind='fandom')`. The query language gets
one new works-only field, `crossover`, with two values: `exclude` (at most one fandom) and `only` (more than one).
`NOT crossover:only` remains the user's generic negation, so no third value is needed. Untagged works count as
non-crossovers, matching the reader meaning "don't show me crossovers" rather than "show me only single-fandom works
with a known fandom".

## Step 1 — parser/AST (`crates/domain/src/query.rs`)

1. Add `Crossover` to `QueryField` after `Completion`.
2. In `as_str`, add `Self::Crossover => "crossover"`.
3. In `parse`, add `"crossover" => Self::Crossover`.
4. In `entity()`, add `Self::Crossover` to the works arm.
5. Leave `is_taxonomy` and `is_comparable` unchanged (a mode, not a name or ordering).

## Step 2 — SQL renderer (`crates/domain/src/query_sql.rs`)

1. In the field `as_str` match, add `QueryField::Crossover => "crossover"`.
2. Add a renderer arm before `Completion`:

```rust
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
```

No binds: the mode is validated into SQL structure. SQLite and PostgreSQL both accept the correlated scalar count.

## Step 3 — dual-engine test (`crates/db/tests/crossover_facet.rs`)

Use `test_support::TestDb::connect_with_dir("fs01", &scratch())`, the local `exec` helper pattern from
`stale_popular_unrated_search.rs`, and `search_works_ast_scoped`. Fixture:

- one account/pseud (`A`, `U`);
- two canonical entities, `F1` and `F2`, `kind='fandom'`;
- work `XC` with both fandom tags; work `SINGLE` with only `F1`.

Assertions:

```rust
let excluded = search(db, "crossover:exclude").await;
assert_eq!(excluded, vec![SINGLE.to_owned()], "exclude must drop the crossover");
let only = search(db, "crossover:only").await;
assert_eq!(only, vec![XC.to_owned()], "only must keep exactly the crossover");
```

Also add a pure parser test in `crates/domain/src/query.rs` tests: `crossover:maybe` is a `QueryError`, and
`crossover:only` parses to `Fielded(QueryField::Crossover, "only")`.

## Step 4 — targeted verification

```bash
cd /home/alvaro/code/rust/darkhold
cargo test -p darkhold-domain crossover
cargo test -p darkhold-db --test crossover_facet                       # SQLite
DARKHOLD_TEST_PG_URL=postgres://darkhold:darkhold@localhost:55433/darkhold \
  cargo test -p darkhold-db --test crossover_facet                     # PostgreSQL
```

Expected: parser tests pass; dual-engine test passes once on each engine.

Mutant guard: temporarily change `"only" => (">", ...)` to `("<=", ...)`, rerun the SQLite test, confirm it fails,
restore, and rerun to green. Record the exact failure line in the handoff.

## Step 5 — UI control (`frontend/src/routes/Search.svelte`)

Add two preset buttons beside the existing "Complete, under 10k" preset, using the same
`query = query.trim() ? `${query.trim()} AND ${term}` : term` pattern as FS-05:

```svelte
<button type="button" class="preset" onclick={() => applyCrossover('exclude')}>
  No crossovers
</button>
<button type="button" class="preset" onclick={() => applyCrossover('only')}>
  Crossovers only
</button>
```

```ts
function applyCrossover(mode: 'exclude' | 'only') {
  const term = `crossover:${mode}`;
  query = query.trim() ? `${query.trim()} AND ${term}` : term;
  void runSearch();
}
```

Add a DOM test asserting the two buttons emit `crossover:exclude` and `crossover:only` as last `searchWorks` calls.
Because the frontend bundle is embedded, rebuild once after both Rust and frontend changes.

Execution note: the PostgreSQL admin URL must point at the existing `postgres` database on port 55433 (`postgres://darkhold:darkhold@localhost:55433/postgres`); pointing it at `darkhold` fails with "database does not exist". The live proof used the server API plus the SPA presets.

## Step 6 — URL/UI proof and state

The server-rendered `/search?q=crossover%3Aonly` and the SPA query box both use this grammar. Prove with:

```bash
curl -s 'https://darkhold.polarisocial.xyz/api/v1/search?q=crossover%3Aonly' | head -c 120
```

Expected: a 200 JSON body with `facets` and `hits` (no 422). Then add a `crossover` preset button to the SPA Search
page only if the field renders without a backend rebuild; do not invent a UI control for a field the server cannot
answer. Mark `FS-01` implemented-verified-e2e, regenerate the ledger counts, move the roadmap row to Done, update
handoff/DECISIONS, and commit.
