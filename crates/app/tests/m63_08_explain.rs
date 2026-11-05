//! `M63-08`: `?explain=1` answers with the engine's plan, and a parse error carries its offset.
//!
//! Two properties, and both are about the *route* rather than the planner:
//!
//! * **the statement explained is the statement the search would run** — same query, same
//!   filters, same limit. A plan for something else is a diagnosis of a query nobody sent.
//! * **a parse failure keeps the parser's position** — the `422` carries `error.query` with the
//!   expression and the character offset, so a terminal can draw a caret instead of asking the
//!   reader to count characters.
//!
//! The planner's own answers are the engine's business and are not asserted here; what is
//! asserted is that they arrive, unaltered, with the engine named.

use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::DatabaseConfig;
use serde_json::Value;
use test_support::{scratch_dir, TestDb};
use tower::ServiceExt;

fn config_for(dir: &Path) -> Config {
    let mut config = Config::development_defaults();
    config.storage.root = dir.to_path_buf();
    // The pool the router queries comes from `TestDb` below, so this value is never
    // connected to — but it must not name the wrong engine. A suite that runs on
    // PostgreSQL while its config says `sqlite://` is exactly what the dialect gate
    // reports, and the gate is right: it cannot tell a dead field from a live one.
    // The selector is read here so the config and the run agree.
    config.database = match std::env::var("DARKHOLD_TEST_PG_URL") {
        Ok(url) if !url.is_empty() => DatabaseConfig::new(url),
        _ => DatabaseConfig::new(format!(
            "sqlite://{}/darkhold.sqlite?mode=rwc",
            dir.display()
        )),
    };
    config
}

struct Harness {
    _dir: PathBuf,
    tdb: TestDb,
    app: axum::Router,
}

impl Harness {
    async fn new(tag: &str) -> Self {
        let dir = scratch_dir(tag);
        let tdb = TestDb::connect_with_dir(tag, &dir).await;
        let app = server::build_router(AppState::new(config_for(&dir), tdb.db().clone()));
        Self {
            _dir: dir,
            tdb,
            app,
        }
    }

    async fn get(&self, uri: &str) -> (StatusCode, Value) {
        let request = Request::builder()
            .method("GET")
            .uri(uri)
            .body(Body::empty())
            .expect("request");
        let response = self
            .app
            .clone()
            .oneshot(request)
            .await
            .expect("the router answers");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("body");
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, body)
    }

    fn engine(&self) -> &'static str {
        if self.tdb.is_postgres() {
            "postgres"
        } else {
            "sqlite"
        }
    }
}

#[tokio::test]
async fn the_plan_is_returned_and_no_hits_are() {
    let h = Harness::new("m63-explain-plan").await;
    let (status, body) = h.get("/api/v1/search?q=tag%3Anaruto&explain=1").await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let explain = &body["explain"];
    assert_eq!(
        explain["engine"].as_str(),
        Some(h.engine()),
        "the answer names the engine that planned it: {explain}"
    );
    let plan = explain["plan"].as_array().expect("plan rows");
    assert!(
        !plan.is_empty(),
        "an empty plan is a walk that collected nothing: {explain}"
    );
    let sql = explain["sql"].as_str().expect("the statement");
    assert!(sql.contains("FROM works"), "{sql}");
    assert!(
        sql.contains("LIMIT"),
        "the explained statement has the limit: {sql}"
    );

    // A diagnosis, not a search: if this ever answers with rows, `explain` becomes a slower
    // way to search and the endpoint does two jobs.
    assert!(body.get("items").is_none(), "{body}");
    assert!(body.get("total").is_none(), "{body}");
}

#[tokio::test]
async fn the_statement_explained_is_the_statement_the_query_renders() {
    // Two different queries, two different statements: if `explain` rendered a fixed
    // statement, the tag below could not appear in it.
    let h = Harness::new("m63-explain-sql").await;
    let (_, tagged) = h.get("/api/v1/search?q=tag%3Anarutox&explain=1").await;
    let (_, plain) = h.get("/api/v1/search?explain=1").await;
    let tagged_sql = tagged["explain"]["sql"].as_str().expect("tagged sql");
    let plain_sql = plain["explain"]["sql"].as_str().expect("plain sql");
    assert!(
        tagged_sql.contains("work_tags"),
        "the tag predicate is in the explained statement: {tagged_sql}"
    );
    assert!(
        !plain_sql.contains("work_tags"),
        "an unfiltered search must not carry the tag join: {plain_sql}"
    );
    assert_ne!(tagged_sql, plain_sql);
}

#[tokio::test]
async fn a_parse_error_carries_the_parser_s_offset() {
    let h = Harness::new("m63-explain-offset").await;
    let expression = "kudos>=100";
    let (status, body) = h.get("/api/v1/search?q=kudos%3E%3D100").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["error"]["code"], "VALIDATION_FAILED", "{body}");

    // The offset is the parser's own, not a number this route computed: the domain is the only
    // thing that knows where the mistake is, and a second guess here would be a second answer.
    let expected = darkhold_domain::query::parse_query(expression)
        .expect_err("this expression does not parse")
        .offset;
    assert_eq!(
        body["error"]["query"]["offset"].as_u64(),
        Some(expected as u64),
        "{body}"
    );
    assert_eq!(body["error"]["query"]["expression"], expression, "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("kudos:>=100")),
        "the sentence names the spelling that works: {body}"
    );
}

#[tokio::test]
async fn a_search_that_is_not_a_parse_error_has_no_query_detail() {
    // A `Validation` from anywhere else must not grow a caret: `error.query` is present only
    // where there is a position, which is the rule `conflict` and `required_scope` follow.
    let h = Harness::new("m63-explain-nodetail").await;
    let (status, body) = h.get("/api/v1/search?q=tag%3Anaruto&limit=0").await;
    // Whatever this answers, it is not a parse failure — assert the shape when it is a 4xx.
    if status.is_client_error() {
        assert!(body["error"].get("query").is_none(), "{body}");
    }
}
