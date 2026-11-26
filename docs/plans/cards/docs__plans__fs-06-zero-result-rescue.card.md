# docs/plans/fs-06-zero-result-rescue.md — plan card

Source: `docs/plans/fs-06-zero-result-rescue.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

Work from `/home/alvaro/code/rust/darkhold`. Spec: `docs/spec-amendments/faceted-search-brief-2026-11-26.md`,
## Step 1 — pure clause splitter (`crates/domain/src/query.rs`)
## Step 2 — route (`crates/app/src/routes/search.rs`)
## Step 3 — app acceptance test (`crates/app/tests/zero_result_rescue.rs`)
## Step 4 — frontend (`frontend/src/lib/api.ts`, `Search.svelte`, `Search.test.ts`)
In `Search.svelte`, extend the zero-result `EmptyState` action snippet with, before the Save button:
cd frontend && node node_modules/vitest/vitest.mjs run src/routes/Search.test.ts

## Copy-pasteable code / commands

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

export interface SearchRelaxation {
  label: string;
  query: string;
  count: number;
}

## Verification command and expected output

and avoids offering an empty "rescue". Cap at three candidates and eight clauses so a hostile query cannot fan out. When one clause is itself impossible, dropping only the other clauses still leaves it; a second pass detects that blocking clause and offers removals that name it plus one more clause.
Add a unit test: quoted `fandom:"A AND B" AND tag:x` yields two clauses; `relaxation_candidates("a AND b AND c")`
## Step 3 — app acceptance test (`crates/app/tests/zero_result_rescue.rs`)
Mutant guard: temporarily invert `if rows.is_empty() { return None; }` in `probe_relaxation`; the test must fail with
only zero-count candidates; restore and rerun green. Measured mutant exit was 101 with
`assertion 'left == right' failed: expected three named removals`.
## Step 4 — frontend (`frontend/src/lib/api.ts`, `Search.svelte`, `Search.test.ts`)
Add a DOM test where `searchWorks` resolves `{ items: [], relaxations: [{ label: 'Remove tag:impossible', query: 'fandom:alpha', count: 2 }] }`:
assert the button appears, clicking sets `searchWorks` last call to `fandom:alpha` and the URL `q` to `fandom:alpha`.
cargo test -p darkhold-domain relaxation
cargo test -p darkhold-app --test zero_result_rescue
cd frontend && node node_modules/vitest/vitest.mjs run src/routes/Search.test.ts

## RED/GREEN evidence

returns rows; mutant must fail.
Mutant guard: temporarily invert `if rows.is_empty() { return None; }` in `probe_relaxation`; the test must fail with
only zero-count candidates; restore and rerun green. Measured mutant exit was 101 with
`assertion 'left == right' failed: expected three named removals`.
