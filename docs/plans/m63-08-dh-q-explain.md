# M63-08 — `dh q --explain`, and a parse error that points at the token

Row: `M63-08` ("dh q --explain prints the query plan and index used; parse errors point at the token
and exit 2"). Area power-user, milestone M63, from the power-user sweep (P8): *slow-query diagnosis
without psql, and a usage error distinguishable from a runtime error by exit code.*

Half of this already exists: a parse failure is a `422` and `dh q` exits `2`. The two things missing
are the **plan** and the **position**.

## Decisions

**1. The plan comes from the engine, not from a model of it.** `--explain` runs
`EXPLAIN QUERY PLAN <statement>` (SQLite) or `EXPLAIN (FORMAT JSON) <statement>` (PostgreSQL) against
the *same statement with the same binds* the search would run. Rejected: printing the SQL alone — that
says what we asked for, not what the engine will do, and the question ("why is this slow") is answered
by the access path. Rejected: a table of "which index each filter uses" — a second model of the
planner, which is exactly the kind of duplicate definition that drifts.

**2. `?explain=1` returns the plan and no hits.** A diagnosis, not a search. Rejected: plan beside
hits — it invites `explain` as a way to search, and it doubles the work of the endpoint to serve a
question about the endpoint.

**3. The parse offset travels as a named field, not as English.** New
`AppError::QueryRejected { message, expression, offset }`, whose `code()` is `VALIDATION_FAILED` and
whose status is `422` — the public contract does not move (precedent: `InvalidCredentials` reports
`AUTH_REQUIRED`), and `ErrorBody` gains `query: { expression, offset }`, serialized only when present,
like `conflict` and `required_scope`. Rejected: leaving the offset inside the message and having the
CLI parse `at offset (\d+)` — the error module's own rule is that a client must not parse English out
of an error. Rejected: a new error *code*: clients switch on codes to decide whether to retry, and a
typo is still a validation failure.

**4. The terminal prints a caret under the expression.** Given `offset`, `dh q` prints

```
kudos>=100
     ^
error: …
```

Rejected: printing the offset as a number — the reader counts characters, which is the work the caret
just did.

## Steps

### Step 1 — `crates/domain/src/error.rs`: the variant and its detail

Add, beside `ConflictDetail`:

```rust
/// Where a query went wrong, for a client that wants to point at it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QueryDetail {
    /// The expression as the caller sent it.
    pub expression: String,
    /// Character offset of the mistake, counted in `chars` so a multi-byte
    /// character is one column.
    pub offset: usize,
}
```

Add the variant after `Validation`:

```rust
    /// The caller's query did not parse.
    ///
    /// A `Validation` in every respect a client can switch on — the code is
    /// `VALIDATION_FAILED` and the status is `422` — but it carries the position of
    /// the mistake as a field. The alternative is a client parsing `at offset 12` out
    /// of an English sentence, which is what the error module exists to stop.
    #[error("{message}")]
    QueryRejected {
        message: String,
        /// The expression as it was sent, so the caret can be drawn under the caller's
        /// own text rather than under a re-rendering of it.
        expression: String,
        offset: usize,
    },
```

Then `code()` → `ErrorCode::ValidationFailed`; `status_code()` → `422` (same arm as `Validation`);
`field_errors()` → empty; `public_message()` unchanged (falls through to `to_string`); and a new
accessor:

```rust
    /// The position of a rejected query's mistake, when there is one.
    pub fn query_detail(&self) -> Option<QueryDetail> {
        match self {
            Self::QueryRejected { expression, offset, .. } => Some(QueryDetail {
                expression: expression.clone(),
                offset: *offset,
            }),
            _ => None,
        }
    }
```

Verification: `cargo test -p darkhold-domain --lib error` — the existing
`codes_are_stable_upper_snake_strings` and envelope tests must still pass, plus one new test asserting
`QueryRejected` reports `VALIDATION_FAILED`, `422`, and a detail with the offset it was given.

### Step 2 — `crates/app/src/http.rs`: the field on the envelope

In `ErrorBody`, after `required_scope`:

```rust
    /// Where a rejected query went wrong (`M63-08`).
    ///
    /// Present only on `QueryRejected`: a caret is worth a field on the one surface
    /// where the caller typed something, and meaningless on every other refusal.
    #[serde(skip_serializing_if = "Option::is_none")]
    query: Option<darkhold_domain::error::QueryDetail>,
```

and in `into_response`, `query: error.query_detail(),`.

Verification: `cargo test -p darkhold-app --lib http` (the envelope-shape test still passes; a new
assertion that a `Validation` error has no `query` key).

### Step 3 — `crates/db/src/search/ast_search.rs`: the plan, from the engine

Factor the statement building out of `search_works_ast_impl` so both the search and the explanation
use one builder:

```rust
/// The statement a search would run, and the binds it would bind, in order.
struct Statement {
    sql: String,
    binds: Vec<String>,
}

fn build_statement(
    db: &Database,
    query: &str,
    viewer_id: Option<&str>,
    limit: i64,
    rules: &[FilterRule],
    scope: &SearchFilters,
) -> Result<Statement> { … }
```

`search_works_ast_impl` becomes: build, then bind and fetch (unchanged behaviour, and its tests are
the proof of that).

Add:

```rust
/// What the engine says it will do with a search, for `dh q --explain` (`M63-08`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchPlan {
    /// `"sqlite"` or `"postgres"` — which planner answered.
    pub engine: &'static str,
    /// The statement the plan is for, with its placeholders as the engine sees them.
    pub sql: String,
    /// One line per plan row: SQLite's `detail` column, or a flattened PostgreSQL
    /// node (`Index Scan using works_updated_idx on works (rows=1 cost=8.3)`).
    pub lines: Vec<String>,
    /// The index names the plan mentions, deduplicated, in plan order.
    pub indexes: Vec<String>,
}

pub async fn explain_search(
    db: &Database,
    query: &str,
    viewer_id: Option<&str>,
    limit: i64,
    rules: &[FilterRule],
    scope: &SearchFilters,
) -> Result<SearchPlan> { … }
```

SQLite: `EXPLAIN QUERY PLAN {sql}` with the same binds, `fetch_all` as `(i64, i64, i64, String)` and
keep the fourth column. Indexes: every `USING INDEX x` / `USING COVERING INDEX x` occurrence, in plan
order.

PostgreSQL: `EXPLAIN (FORMAT JSON) {sql}`, one row of `serde_json::Value`; flatten the `Plan` tree
depth-first into `"{Node Type} on {Relation Name}"` plus `using {Index Name}` when present, plus
`(cost=… rows=…)`. Indexes: every `Index Name` value in the tree.

Verification: `cargo test -p darkhold-db --test search_plan` — SQLite arm asserts `engine == "sqlite"`,
`lines` is not empty and the statement contains `FROM works`; the PostgreSQL arm runs only under
`DARKHOLD_TEST_PG_URL` and asserts `engine == "postgres"` and that some line names a `Scan`.

### Step 4 — `crates/app/src/routes/search.rs`: `?explain=1`, and the parse mapping

`SearchQuery` gains `#[serde(default)] explain: bool`. In `search()`, after the filters and scope are
resolved and **before** any history write:

```rust
    if params.explain {
        let plan = explain_search(…).await.map_err(search_failure)?;
        return Ok(Json(serde_json::json!({
            "explain": {
                "engine": plan.engine,
                "sql": plan.sql,
                "plan": plan.lines,
                "indexes": plan.indexes,
            }
        })));
    }
```

`search_failure` maps a parse failure to `AppError::QueryRejected` carrying the expression:

```rust
        Some(search) if matches!(search.problem, QueryProblem::Parse { .. }) => {
            let offset = search.problem.offset().unwrap_or(0);
            ApiError(AppError::QueryRejected {
                message: search.problem.to_string(),
                expression: expression.to_owned(),
                offset,
            })
        }
```

which means `search_failure` takes the expression as an argument.

Verification: `cargo test -p darkhold-app --test m63_08_explain` — a 200 whose body has `explain.plan`
non-empty, `explain.engine`, no `hits`; and a 422 for `kudos>=100` whose
`error.query.offset == 6` and `error.query.expression == "kudos>=100"`.

### Step 5 — `crates/cli/src/q.rs`: `--explain`, and the caret

`QArgs` gains:

```rust
    /// Print the query plan and the indexes the engine would use, instead of hits.
    ///
    /// The plan is the engine's own (`EXPLAIN QUERY PLAN` / `EXPLAIN (FORMAT JSON)`),
    /// run against the statement the search would execute.
    #[arg(long)]
    pub explain: bool,
```

`url()` gains `explain: bool` and appends `&explain=1`. `shape()` refuses `--explain` with `--jsonl`,
`--cols`, `--fields` and `--wide` (nothing to project, and `--json` stays meaningful: it prints the
server's `explain` object).

`render_plan(&body) -> String`: `engine: …`, the statement, `plan:` with each line indented, and
`indexes: a, b` (or `indexes: none — this statement is a scan`).

`caret(expression, offset) -> String`: the expression, then spaces to the offset, then `^`. Counted in
`chars`, matching the server's offset. On a `422` whose body has `error.query`, `run()` prints the
caret to stderr **above** the server's sentence and still returns `Failure::Usage` (exit 2).

Verification: `cargo test -p darkhold-cli --lib q` — the flag parses; `--explain --jsonl` is refused
with a message naming both; `caret("kudos>=100", 6)` is `"     ^"`; the plan renderer turns a body into
three labelled blocks. Then against a real instance:

```bash
dh q 'tag:naruto' --explain
dh q 'kudos>=100' ; echo "exit=$?"     # caret, then exit 2
```

### Step 6 — the record

`README.md` gains the `--explain` section beside `dh q`'s grammar. `docs/requirements.csv` row
`M63-08` → `implemented-fully-tested` with the evidence strings above. No new `ROUTE_TABLE` row: the
path is unchanged and `explain` is a query parameter.

## Verification, in order

```bash
cargo test -p darkhold-domain --lib error        # the variant's code, status, detail
cargo test -p darkhold-db --test search_plan     # SQLite arm
DARKHOLD_TEST_PG_URL=postgres://…@127.0.0.1:55433/postgres cargo test -p darkhold-db --test search_plan
cargo test -p darkhold-app --test m63_08_explain
cargo test -p darkhold-cli --lib q
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings
bash scripts/gate.sh ledger && bash scripts/gate.sh repo
```

Each fix is mutation-proved: deleting the `explain` branch must fail
`m63_08_explain::the_plan_is_returned_and_no_hits_are`; returning `QueryRejected` without the offset
must fail the caret test; and swapping the statement under `EXPLAIN` for a hand-written one must fail
the assertion that the plan's SQL is the search's SQL.
