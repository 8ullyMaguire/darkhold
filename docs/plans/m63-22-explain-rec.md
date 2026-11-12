# M63-22 — `dh admin explain-rec <work>`, the operator's microscope

Status: **plan, not built.** Written 2026-11-12 after the census, while a workspace test run held the box.

## Problem

An operator asking "why is this work ranked first" has two partial answers today and neither is the one they need.
`GET /discovery/slots/{slot_id}/explanation` explains a *served slot* to the reader who was served it, which means it
only exists for a work that already appeared. `RecRegistry::generate_traced` produces every strategy's ranking for a
*feed request*, but nothing exposes it and nothing points it at one work. So the question that actually gets asked —
"for this account, how did each strategy score this work, and what does the work look like" — has no surface.

## Non-goals

- Not a reader feature. It exposes an account's taste as a per-strategy table; it is `Audience::Operator` and stays there.
- Not a new scoring implementation. The number it prints is the number the blend used, or it is not printed.
- Not `dh admin health` or `dh admin backup` (M63-23, demoted — see roadmap).
- No caching. A trace is a debugging tool whose cost is acceptable; a cached one would answer about the past.

## Census (§2.2)

**REUSE `RecRegistry::generate_traced` → `RecRunReport` → `StrategyContribution { name, produced, ranked: Vec<(work_id, rrf)> }`
(`crates/db/src/rec_strategy.rs:41-61`), `db::works` for the feature vector, `operator_only` + `Audience::Operator`
(`crates/app/src/routes/identity_admin.rs:122-128`) for the door, and the `admin` subcommand scaffolding. ADAPT
`account_facts_route` for the handler shape. BUILD only the route, the per-work projection and the CLI verb.**

## Success criteria

1. `GET /api/v1/admin/accounts/{account}/explain-rec/{work}` returns, for every registered strategy: its name, how many
   works it produced, whether the target work is in its ranking, that rank (1-based), and the RRF contribution the blend
   used — or `null` for each of those when the strategy did not return the work.
2. The same response carries the work's feature vector: `words`, `chapters`, `kudos`, `hits`, `bookmarks`, `comments`,
   `rating`, `complete`, `published_at`, `age_days`, and its tags.
3. `blended_position` says where the work lands in the blend, so the operator can see a work that every strategy likes
   and the blend buries.
4. A non-operator session gets `403 ACCESS_DENIED`; an unknown work gets `404`; an unknown account gets `404`.
5. `dh admin explain-rec <work>` prints the same data as a table, and `--json` prints the body unchanged.
6. Both engines: the suite passes on SQLite and with `DARKHOLD_TEST_PG_URL` set.

## Files

| file | change |
|---|---|
| `crates/db/src/rec_strategy.rs` | `pub fn explain_work(report: &RecRunReport, work: &str) -> WorkExplanation` — pure projection over the report, no DB |
| `crates/app/src/routes/discovery_admin.rs` | `explain_rec_route` (new) |
| `crates/app/src/routes/inventory.rs` | one `RouteEntry` (below) |
| `crates/app/src/server.rs` | mount the route under `/api/v1` |
| `crates/cli/src/admin.rs` | new module: `AdminArgs { command: AdminCommand }`, `AdminCommand::ExplainRec { work: String, account: String }` |
| `crates/cli/src/main.rs` | `mod admin;`, `Command::Admin(admin::AdminArgs)`, dispatch |
| `crates/app/tests/m63_22_explain_rec.rs` | the failing-first suite |
| `crates/cli/tests/m63_22_explain_rec_cli.rs` | the verb's own test, against a stub body |

## The projection

In `crates/db/src/rec_strategy.rs`, beside `RecRunReport`:

```rust
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
```

## The route

`RouteEntry` in `crates/app/src/routes/inventory.rs`, alphabetically beside the other `/admin/accounts/…` rows:

```rust
RouteEntry {
    file: "discovery_admin.rs",
    handler: "explain_rec_route",
    method: "GET",
    path: "/admin/accounts/{id}/explain-rec/{work}",
    audience: Audience::Operator,
},
```

Handler in `crates/app/src/routes/discovery_admin.rs`:

```rust
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
```

The limit of 200 is deliberate and must be stated in the response as `"limit": 200`: a trace over an unbounded feed is a
benchmark of the catalogue wearing a microscope's name, and a work below the cut is honestly `null` rather than absent.

## Tests (failing first)

`crates/app/tests/m63_22_explain_rec.rs`:

1. `an_operator_sees_every_strategy_and_where_the_work_landed` — seed three works and two accounts with taste, run the
   route, assert: every registry strategy name appears; for the work that the cooccurrence strategy returns, `rank >= 1`
   and `rrf > 0.0`; `blended_position` is consistent with the order the feed route returns.
2. `a_strategy_that_never_looked_is_null_and_not_zero` — for a work nobody's taste touches, assert `rank: null`,
   `rrf: null` on every strategy, and that the JSON has no `0.0` in those slots.
3. `a_reader_is_refused_and_the_sentence_says_access_denied` — `403`, body `error.code == "ACCESS_DENIED"`.
4. `an_unknown_work_is_a_404_not_an_empty_strategy_table` — asserting `404` and that the body is not the happy path with
   every field null.

`crates/cli/tests/m63_22_explain_rec_cli.rs`: the verb, against a recorded body — asserts the table has one row per
strategy, a `-` where rank is null (never `0`), and that `--json` prints the body byte-for-byte.

## Verification

```
$ env -u DATABASE_URL -u DARKHOLD_TEST_PG_URL cargo test -p darkhold-app --test m63_22_explain_rec
test result: ok. 4 passed; 0 failed
$ DARKHOLD_TEST_PG_URL=postgres://darkhold:darkhold@127.0.0.1:55433/postgres \
  cargo test -p darkhold-app --test m63_22_explain_rec
test result: ok. 4 passed; 0 failed
$ cargo test -p darkhold-app --test route_inventory
test result: ok. 6 passed; 0 failed        # the new RouteEntry is reachable and its extractors resolve
$ cargo test -p darkhold-cli --bins
test result: ok.
```

**Mutant**, reverted from `/tmp`: in `explain_work`, replace `.position(|(candidate, _)| candidate == work)` with
`.position(|_| true)` — the "never looked" test must go red on `rank: null, rrf: null`, and if it stays green the test is
asserting the happy path twice.

## Ledger

`python3 scripts/ledger.py upsert` with `status=implemented-fully-tested`, `gates` = the four commands above,
`evidence=crates/app/tests/m63_22_explain_rec.rs (4 tests) + crates/cli/tests/m63_22_explain_rec_cli.rs`.
