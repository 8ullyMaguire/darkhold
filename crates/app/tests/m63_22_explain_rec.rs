//! M63-22 — the operator's microscope: one work, seen through every strategy.
//!
//! Failing first: before the route exists every case answers 404 from the SPA fallback, which is
//! exactly what an unmounted JSON surface looks like. The reader case uses a site moderator so the
//! refusal is 403 (a caller with no standing at all is answered 404 by ADR 0030 — that is a *different*
//! refusal and would prove nothing about this door).

use serde_json::Value;

use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::Database;

use test_support::{scratch_dir, sign_in_as, TestClient, TestDb};

const ROUTE: &str = "/api/v1/admin/discovery/explain-rec";

async fn set_roles(db: &Database, roles: &str) {
    let sql = db.sql(
        "UPDATE sessions SET roles = ?",
        "UPDATE sessions SET roles = $1",
    );
    match db.backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(roles)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("setting roles");
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(&sql)
                .bind(roles)
                .execute(db.postgres_pool().expect("postgres"))
                .await
                .expect("setting roles");
        }
    }
}

async fn insert_work(db: &Database, id: &str, handle: &str) {
    let (sql, owner) = match db.backend() {
        darkhold_db::Backend::Sqlite => (
            "INSERT INTO works (id, owner_pseud_id, title, summary, rating, lifecycle, visibility, completion, published_at, created_at, updated_at) \
             SELECT ?, id, 'A work', 'summary', 'teen', 'published', 'public', 'complete', datetime('now'), datetime('now'), datetime('now') \
             FROM pseuds WHERE handle = ?",
            None,
        ),
        darkhold_db::Backend::Postgres => (
            "INSERT INTO works (id, owner_pseud_id, title, summary, rating, lifecycle, visibility, completion, published_at, created_at, updated_at) \
             SELECT $1::uuid, id, 'A work', 'summary', 'teen', 'published', 'public', 'complete', now(), now(), now() \
             FROM pseuds WHERE handle = $2",
            Some(()),
        ),
    };
    let _ = owner;
    match db.backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(sql)
                .bind(id)
                .bind(handle)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("inserting work");
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(sql)
                .bind(id)
                .bind(handle)
                .execute(db.postgres_pool().expect("postgres"))
                .await
                .expect("inserting work");
        }
    }
}

async fn setup(tag: &str, roles: &str) -> (TestClient, TestDb, String, String) {
    let dir = scratch_dir(tag);
    let mut config = Config::development_defaults();
    config.storage.root = dir.clone();
    config.administration.operator_account_id = None;
    let tdb = TestDb::connect_with_dir(tag, &dir).await;
    let db = tdb.db().clone();
    let mut client = TestClient::new(server::build_router(AppState::new(config, db.clone())));
    let account = sign_in_as(&mut client, &tdb, "operator@example.com", "operator").await;
    let reader = sign_in_as(&mut client, &tdb, "reader@example.com", "reader").await;
    set_roles(&db, roles).await;
    insert_work(&db, "11111111-1111-4111-8111-111111111111", "reader").await;
    (client, tdb, account, reader)
}

#[tokio::test]
async fn an_operator_sees_every_strategy_and_the_work_features() {
    let (mut client, _tdb, account, _reader) =
        setup("m63_22_operator", r#"["role:operator"]"#).await;
    let (status, body) = client
        .get(&format!(
            "{ROUTE}?account={account}&work=11111111-1111-4111-8111-111111111111"
        ))
        .await;
    assert_eq!(status.as_u16(), 200, "route must exist: {status} {body}");
    let json: Value = body.clone();
    assert_eq!(json["limit"], 200, "a trace must name its cut: {body}");
    assert!(
        json["strategies"].as_array().is_some_and(|a| !a.is_empty()),
        "every registered strategy must appear: {body}"
    );
    let features = &json["features"];
    for field in [
        "words",
        "chapters",
        "kudos",
        "hits",
        "bookmarks",
        "comments",
        "rating",
        "complete",
        "age_days",
        "tags",
    ] {
        assert!(
            !features[field].is_null(),
            "missing feature {field}: {body}"
        );
    }
    assert!(
        json.get("blended_position").is_some(),
        "blend position: {body}"
    );
}

#[tokio::test]
async fn a_strategy_that_never_looked_is_null_and_not_zero() {
    let (mut client, _tdb, account, _reader) = setup("m63_22_null", r#"["role:operator"]"#).await;
    let (status, body) = client
        .get(&format!(
            "{ROUTE}?account={account}&work=11111111-1111-4111-8111-111111111111"
        ))
        .await;
    assert_eq!(status.as_u16(), 200, "{body}");
    let json: Value = body.clone();
    let strategies = json["strategies"].as_array().expect("strategies");
    // `bandit` explores an untasted catalogue, so not every strategy is absent here. What the row must prove is
    // the two-sided rule: a strategy that returned the work has a positive contribution, and one that did not
    // has null rather than a zero that would say it weighed the work and found it worthless.
    assert!(
        strategies.iter().any(|s| s["rank"].is_null()),
        "at least one strategy must have missed the work: {body}"
    );
    assert!(
        strategies.iter().any(|s| s["rank"].is_number()),
        "at least one strategy must have returned it: {body}"
    );
    for strategy in strategies {
        match (&strategy["rank"], &strategy["rrf"]) {
            (Value::Null, Value::Null) => {}
            (Value::Number(rank), Value::Number(rrf)) => {
                assert!(
                    rank.as_u64().is_some_and(|n| n >= 1),
                    "1-based rank: {strategy}"
                );
                assert!(
                    rrf.as_f64().is_some_and(|n| n > 0.0),
                    "positive contribution: {strategy}"
                );
            }
            _ => panic!("rank and rrf must agree on whether the strategy looked: {strategy}"),
        }
    }
    assert!(
        !body.to_string().contains("\"rank\":0"),
        "a zero rank leaked into the body: {body}"
    );
}

#[tokio::test]
async fn a_reader_is_refused_with_access_denied() {
    let (mut client, _tdb, account, _reader) =
        setup("m63_22_refusal", r#"["role:moderator:darkhold"]"#).await;
    let (status, body) = client
        .get(&format!(
            "{ROUTE}?account={account}&work=11111111-1111-4111-8111-111111111111"
        ))
        .await;
    assert_eq!(
        status.as_u16(),
        403,
        "a moderator is not an operator: {status} {body}"
    );
    let json: Value = body.clone();
    assert_eq!(json["error"]["code"], "ACCESS_DENIED", "{body}");
}

#[tokio::test]
async fn an_unknown_work_is_a_404_not_a_null_strategy_table() {
    let (mut client, _tdb, account, _reader) =
        setup("m63_22_missing", r#"["role:operator"]"#).await;
    let (status, body) = client
        .get(&format!(
            "{ROUTE}?account={account}&work=00000000-0000-4000-8000-000000000000"
        ))
        .await;
    assert_eq!(
        status.as_u16(),
        404,
        "an unknown work must be 404: {status} {body}"
    );
    assert!(
        !body.to_string().contains("strategies"),
        "a 404 must not be a happy path: {body}"
    );
}

#[tokio::test]
async fn the_feature_query_runs_on_both_engines() {
    let (_client, tdb, _account, _reader) = setup("m63_22_features", r#"["role:operator"]"#).await;
    let features =
        darkhold_db::rec_strategy::work_features(tdb.db(), "11111111-1111-4111-8111-111111111111")
            .await
            .expect("the feature query must run on both engines");
    assert_eq!(features.words, 0);
    assert_eq!(features.chapters, 0);
    assert!(features.complete, "the fixture work is complete");
    assert!(features.published_at.is_some());
}
