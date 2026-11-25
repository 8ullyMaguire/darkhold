# FS-06 — zero-result rescue

Work from `/home/alvaro/code/rust/darkhold`. Spec: `docs/spec-amendments/faceted-search-brief-2026-11-26.md`,
`FS-06`. Acceptance: an impossible tag query returns three named clause removals; clicking one changes the URL and
returns rows; mutant must fail.

## Decision

Relaxations are computed server-side only for a zero-hit query. Each removable top-level AND clause is removed in
turn; a candidate is offered only when the shortened query actually returns rows. This avoids client-side guessing
and avoids offering an empty "rescue". Cap at three candidates and eight clauses so a hostile query cannot fan out.
The label names the removed clause (`Remove tag:impossible`) and the candidate query is copied verbatim into the URL
on click, reusing the existing query/URL round trip from FS-05.

## Step 1 — pure clause splitter (`crates/domain/src/query.rs`)

```rust
/// Split a canonical query on top-level ` AND ` separators, respecting double quotes.
pub fn and_clauses(query: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut chars = query.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '"' {
            quoted = !quoted;
            current.push(c);
            continue;
        }
        if !quoted && c == 'A' {
            let rest: String = chars.clone().take(3).collect();
            if rest == "ND " {
                chars.next(); chars.next(); chars.next();
                if !current.trim().is_empty() { parts.push(current.trim().to_owned()); }
                current.clear();
                continue;
            }
        }
        current.push(c);
    }
    if !current.trim().is_empty() { parts.push(current.trim().to_owned()); }
    parts
}

/// (label, relaxed query) for dropping one clause at a time.
pub fn relaxation_candidates(query: &str) -> Vec<(String, String)> {
    let clauses = and_clauses(query);
    if clauses.len() < 2 { return Vec::new(); }
    clauses.iter().enumerate().take(8).map(|(i, dropped)| {
        let rest = clauses.iter().enumerate()
            .filter(|(j, _)| *j != i).map(|(_, c)| c.as_str())
            .collect::<Vec<_>>().join(" AND ");
        (format!("Remove {dropped}"), rest)
    }).collect()
}
```

Add a unit test: quoted `fandom:"A AND B" AND tag:x` yields two clauses; `relaxation_candidates("a AND b AND c")`
yields labels `Remove a`, `Remove b`, `Remove c` and queries `b AND c`, `a AND c`, `a AND b`.

## Step 2 — route (`crates/app/src/routes/search.rs`)

Add near the response:

```rust
#[derive(Debug, serde::Serialize)]
pub struct SearchRelaxation {
    pub label: String,
    pub query: String,
    pub count: usize,
}

async fn zero_result_relaxations(
    state: &AppState,
    q: &str,
    viewer_id: Option<&str>,
    filters: &darkhold_db::search::SearchFilters,
    scope: &darkhold_db::search::SearchFilters,
) -> Vec<SearchRelaxation> {
    let mut out = Vec::new();
    for (label, candidate) in darkhold_domain::query::relaxation_candidates(q) {
        let rows = match search_works_ast_scoped(
            state.db(), &candidate, viewer_id, 20, &filters, scope,
        ).await {
            Ok(rows) => rows,
            Err(_) => continue,
        };
        if rows.is_empty() { continue; }
        out.push(SearchRelaxation { label, query: candidate, count: rows.len() });
        if out.len() == 3 { break; }
    }
    out
}
```

After `let results = ...`, compute `relaxations` when `results.is_empty()` and add `"relaxations": relaxations` to
the JSON response. Keep the main search unchanged.

## Step 3 — app acceptance test (`crates/app/tests/zero_result_rescue.rs`)

Build a `TestDb`, insert an account/pseud, three works tagged `fandom:alpha`, `fandom:beta` and `tag:good`, then:

1. `GET /api/v1/search?q=tag:impossible%20AND%20fandom%3Aalpha%20AND%20tag%3Agood` -> `items=[]`,
   `relaxations` contains exactly the labels for `tag:impossible` and any other non-empty removals, max 3.
2. Every returned `query` is a clause-removal string and contains no `tag:impossible`.
3. Follow the first relaxation query -> `items` non-empty.

Mutant guard: temporarily invert `if rows.is_empty() { continue; }` to `if !rows.is_empty() { continue; }`; the test
must fail; restore and rerun green.

## Step 4 — frontend (`frontend/src/lib/api.ts`, `Search.svelte`, `Search.test.ts`)

API type:

```ts
export interface SearchRelaxation {
  label: string;
  query: string;
  count: number;
}
```

add `relaxations?: SearchRelaxation[]` to `SearchResultList`.

In `Search.svelte`, extend the zero-result `EmptyState` action snippet with, before the Save button:

```svelte
{#each results?.relaxations ?? [] as relaxation (relaxation.query)}
  <button
    type="button"
    class="relax"
    onclick={() => {
      query = relaxation.query;
      void runSearch();
    }}
  >
    {relaxation.label} ({relaxation.count})
  </button>
{/each}
```

Add a DOM test where `searchWorks` resolves `{ items: [], relaxations: [{ label: 'Remove tag:impossible', query: 'fandom:alpha', count: 2 }] }`:
assert the button appears, clicking sets `searchWorks` last call to `fandom:alpha` and the URL `q` to `fandom:alpha`.

## Step 5 — verification and state

```bash
cargo test -p darkhold-domain relaxation
cargo test -p darkhold-app --test zero_result_rescue
cd frontend && node node_modules/vitest/vitest.mjs run src/routes/Search.test.ts
```

Build/deploy only if the bundle or binary changed. Live proof: impossible query returns named relaxations and clicking
one returns rows. Mark FS-06 done in requirements/ledger/roadmap/handoff/DECISIONS and commit.
