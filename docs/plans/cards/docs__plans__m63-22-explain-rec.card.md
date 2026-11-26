# docs/plans/m63-22-explain-rec.md — plan card

Source: `docs/plans/m63-22-explain-rec.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

Status: **built 2026-11-16.** The file names below were a proposal; the route landed in the existing `admin_discovery.rs` (there was no `discovery_admin.rs` in this tree) and the query-string selectors replaced the two-id path. See the decision record in `roadmap-remaining.md`.
(`crates/db/src/rec_strategy.rs:41-61`), `db::works` for the feature vector, `operator_only` + `Audience::Operator`
(`crates/app/src/routes/identity_admin.rs:122-128`) for the door, and the `admin` subcommand scaffolding. ADAPT
| `crates/db/src/rec_strategy.rs` | `pub fn explain_work(report: &RecRunReport, work: &str) -> WorkExplanation` — pure projection over the report, no DB |
| `crates/app/src/routes/discovery_admin.rs` | `explain_rec_route` (new) |
| `crates/app/src/routes/inventory.rs` | one `RouteEntry` (below) |
| `crates/app/src/server.rs` | mount the route under `/api/v1` |
| `crates/cli/src/admin.rs` | new module: `AdminArgs { command: AdminCommand }`, `AdminCommand::ExplainRec { work: String, account: String }` |
| `crates/cli/src/main.rs` | `mod admin;`, `Command::Admin(admin::AdminArgs)`, dispatch |
| `crates/app/tests/m63_22_explain_rec.rs` | the failing-first suite |
| `crates/cli/tests/m63_22_explain_rec_cli.rs` | the verb's own test, against a stub body |
In `crates/db/src/rec_strategy.rs`, beside `RecRunReport`:

## Copy-pasteable code / commands

/// Where one work sits in a run, strategy by strategy.
///
/// `ranked` is `(work_id, rrf)` best-first, so both numbers are the run's own: `rank` is the position a reader would
/// have seen, `rrf` is the contribution the blend added. A strategy that never returned the work contributes nothing,
/// which is `None` here and must not be reported as a zero score — "0.0" would say the strategy weighed it and found
/// it worthless, and the truth is that the strategy never looked.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkInStrategy {
    pub name: String,
    pub produced: usize,
    pub rank: Option<usize>,
    pub rrf: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorkExplanation {
    pub blended_position: Option<usize>,
    pub strategies: Vec<WorkInStrategy>,
}

pub fn explain_work(report: &RecRunReport, work: &str) -> WorkExplanation {
    WorkExplanation {
        blended_position: report
            .blended
            .iter()
            .position(|candidate| candidate == work)
            .map(|index| index + 1),
        strategies: report
            .per_strategy
            .iter()
            .map(|contribution| {
                let hit = contribution
                    .ranked
                    .iter()
                    .position(|(candidate, _)| candidate == work);
                WorkInStrategy {
                    name: contribution.name.clone(),
                    produced: contribution.produced,
                    rank: hit.map(|index| index + 1),
                    rrf: hit.map(|index| contribution.ranked[index].1),
                }
            })
            .collect(),
    }
}

RouteEntry {
    file: "discovery_admin.rs",
    handler: "explain_rec_route",
    method: "GET",
    path: "/admin/accounts/{id}/explain-rec/{work}",
    audience: Audience::Operator,
},

/// `GET /admin/accounts/{id}/explain-rec/{work}` — one work, seen through every strategy (M63-22).
pub async fn explain_rec_route(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path((id, work)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    operator_only(&state, &user)?;
    let account = require_account(&state, &id).await?;
    let work_row = require_work(&state, &work).await?; // 404 with the same sentence the work page uses
    let report = state
        .rec
        .generate_traced(state.db(), RecContext::for_account(&account).with_limit(200))
        .await?;
    let explanation = explain_work(&report, &work);
    Ok(Json(json!({ "account": id, "work": work_row_json(&work_row), "blended_position": explanation.blended_position,
                     "strategies": explanation.strategies.iter().map(|s| json!({ "name": s.name, "produced": s.produced,
                         "rank": s.rank, "rrf": s.rrf })).collect::<Vec<_>>() })))
}

## Verification command and expected output

| `crates/cli/tests/m63_22_explain_rec_cli.rs` | the verb's own test, against a stub body |
   route, assert: every registry strategy name appears; for the work that the cooccurrence strategy returns, `rank >= 1`
2. `a_strategy_that_never_looked_is_null_and_not_zero` — for a work nobody's taste touches, assert `rank: null`,
$ env -u DATABASE_URL -u DARKHOLD_TEST_PG_URL cargo test -p darkhold-app --test m63_22_explain_rec
test result: ok. 4 passed; 0 failed
  cargo test -p darkhold-app --test m63_22_explain_rec
test result: ok. 4 passed; 0 failed
$ cargo test -p darkhold-app --test route_inventory
test result: ok. 6 passed; 0 failed        # the new RouteEntry is reachable and its extractors resolve
$ cargo test -p darkhold-cli --bins
test result: ok.
`.position(|_| true)` — the "never looked" test must go red on `rank: null, rrf: null`, and if it stays green the test is

## RED/GREEN evidence

test result: ok. 4 passed; 0 failed
test result: ok. 4 passed; 0 failed
test result: ok. 6 passed; 0 failed        # the new RouteEntry is reachable and its extractors resolve
**Mutant**, reverted from `/tmp`: in `explain_work`, replace `.position(|(candidate, _)| candidate == work)` with
`.position(|_| true)` — the "never looked" test must go red on `rank: null, rrf: null`, and if it stays green the test is
