# docs/plans/m63-08-dh-q-explain.md — plan card

Source: `docs/plans/m63-08-dh-q-explain.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

### Step 1 — `crates/domain/src/error.rs`: the variant and its detail
### Step 2 — `crates/app/src/http.rs`: the field on the envelope
### Step 3 — `crates/db/src/search/ast_search.rs`: the plan, from the engine
### Step 4 — `crates/app/src/routes/search.rs`: `?explain=1`, and the parse mapping
                "sql": plan.sql,
### Step 5 — `crates/cli/src/q.rs`: `--explain`, and the caret
`README.md` gains the `--explain` section beside `dh q`'s grammar. `docs/requirements.csv` row
bash scripts/gate.sh ledger && bash scripts/gate.sh repo

## Copy-pasteable code / commands

kudos>=100
     ^
error: …

/// Where a query went wrong, for a client that wants to point at it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QueryDetail {
    /// The expression as the caller sent it.
    pub expression: String,
    /// Character offset of the mistake, counted in `chars` so a multi-byte
    /// character is one column.
    pub offset: usize,
}

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

## Verification command and expected output

and exit 2"). Area power-user, milestone M63, from the power-user sweep (P8): *slow-query diagnosis
without psql, and a usage error distinguishable from a runtime error by exit code.*
Verification: `cargo test -p darkhold-domain --lib error` — the existing
`codes_are_stable_upper_snake_strings` and envelope tests must still pass, plus one new test asserting
Verification: `cargo test -p darkhold-app --lib http` (the envelope-shape test still passes; a new
Verification: `cargo test -p darkhold-db --test search_plan` — SQLite arm asserts `engine == "sqlite"`,
Verification: `cargo test -p darkhold-app --test m63_08_explain` — a 200 whose body has `explain.plan`
caret to stderr **above** the server's sentence and still returns `Failure::Usage` (exit 2).
Verification: `cargo test -p darkhold-cli --lib q` — the flag parses; `--explain --jsonl` is refused
dh q 'kudos>=100' ; echo "exit=$?"     # caret, then exit 2
cargo test -p darkhold-domain --lib error        # the variant's code, status, detail
cargo test -p darkhold-db --test search_plan     # SQLite arm
DARKHOLD_TEST_PG_URL=postgres://…@127.0.0.1:55433/postgres cargo test -p darkhold-db --test search_plan
cargo test -p darkhold-app --test m63_08_explain
cargo test -p darkhold-cli --lib q
bash scripts/gate.sh ledger && bash scripts/gate.sh repo
must fail the caret test; and swapping the statement under `EXPLAIN` for a hand-written one must fail

## RED/GREEN evidence

No RED/GREEN evidence was recorded in the source document.
