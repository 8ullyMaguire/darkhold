//! Quality signals: a search dimension with no writer (M70-13).
//!
//! §32.1 defines `quality_signals` as "typed, sourced, recomputable, 0..=1000" and §32.3 makes quality a *filter dimension*:
//! `min_quality:750` and `quality:editorial_review:750` are part of the query language, and the "Hidden gems" recommendation
//! surface declares `quality_signals` as a data need. All of that exists. What did not exist was a writer: the only INSERT of a
//! signal in the whole tree is in `milestone_22`'s own fixture (`source = 'test'`), and the deployed table holds **0 rows**.
//!
//! The consequence is a filter that always answers "nothing": on the live instance every `min_quality` search is empty, which reads
//! to the person typing it as "no such work" rather than "nothing is scored". A ranking input no operator can write is a dimension
//! that silently excludes the entire catalogue.
//!
//! This test drives the door an operator actually needs — a recorded, attributed judgement — and then asks the *search* surface,
//! because a signal nothing can query is not a feature:
//!
//! * `POST /admin/works/{id}/quality-signals` writes one signal with the acting account as its `source`, and answers with the row;
//! * `GET /works/{id}/quality` reads them back with the weighted composite the filter uses — public, because the filter exposes the
//!   aggregate anyway and hiding the components while filtering on them is the worse half of that choice;
//! * a value outside 0..=1000, or a kind the vocabulary does not name, is `422` naming the field;
//! * a caller without standing gets `404`, like every other admin door (ADR 0003);
//! * and `min_quality:750` in the real search returns the scored work and not the unscored one — the chain from door to reader.

use axum::http::StatusCode;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::Database;
use serde_json::json;
use test_support::{register, scratch_dir, sign_in_as, TestClient, TestDb};

fn build(config: &Config, db: &Database) -> axum::Router {
    server::build_router(AppState::new(config.clone(), db.clone()))
}

async fn account_id(db: &Database, handle: &str) -> uuid::Uuid {
    let sql = db.sql(
        "SELECT account_id FROM pseuds WHERE handle = ?",
        "SELECT account_id::text FROM pseuds WHERE handle = $1",
    );
    let raw: String = match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar(&sql)
            .bind(handle)
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("account for handle"),
        darkhold_db::Backend::Postgres => sqlx::query_scalar(&sql)
            .bind(handle)
            .fetch_one(db.postgres_pool().expect("postgres"))
            .await
            .expect("account for handle"),
    };
    raw.parse().expect("uuid account id")
}

async fn publish_work(author: &mut TestClient, title: &str) -> String {
    let (status, work) = author.post("/api/v1/works", json!({ "title": title })).await;
    assert_eq!(status, StatusCode::CREATED, "{work}");
    let work_id = work["id"].as_str().expect("work id").to_owned();
    let (status, unit) = author
        .post(
            &format!("/api/v1/works/{work_id}/chapters"),
            json!({ "title": "One" }),
        )
        .await;
    assert!(status.is_success(), "unit: {unit}");
    let unit_id = unit["id"].as_str().expect("unit id").to_owned();
    let (status, body) = author
        .request(
            "PATCH",
            &format!("/api/v1/chapters/{unit_id}"),
            Some(json!({
                "expected_version": unit["version"].as_i64().unwrap_or(1),
                "document": {
                    "type": "doc",
                    "content": [{
                        "type": "paragraph",
                        "content": [{ "type": "text", "text": "Words a reader can find." }]
                    }]
                }
            })),
        )
        .await;
    assert!(status.is_success(), "write unit: {body}");
    let (status, current) = author.get(&format!("/api/v1/works/{work_id}")).await;
    assert_eq!(status, StatusCode::OK, "{current}");
    let version = current["version"].as_i64().unwrap_or(1);
    let (status, body) = author
        .post(
            &format!("/api/v1/works/{work_id}/publish"),
            json!({ "expected_version": version }),
        )
        .await;
    assert!(status.is_success(), "publish: {body}");
    work_id
}

#[tokio::test]
async fn an_operator_records_a_quality_signal_and_the_search_filter_finds_it() {
    let dir = scratch_dir("quality-signals");
    let tdb = TestDb::connect_with_dir("m7013-quality-signals", &dir).await;
    let db: Database = tdb.db().clone();
    let mut config = Config::development_defaults();
    config.storage.root = dir.clone();

    // The operator must exist *before* the config names them, because the admin gate compares the session against
    // `config.administration.operator_account_id`.
    let mut bootstrap = TestClient::new(build(&config, &db));
    register(&mut bootstrap, "qs-operator@example.test", "qsoperator").await;
    config.administration.operator_account_id = Some(account_id(&db, "qsoperator").await.into());

    let mut operator = TestClient::new(build(&config, &db));
    sign_in_as(&mut operator, &tdb, "qs-operator@example.test", "qsoperator").await;

    let mut author = TestClient::new(build(&config, &db));
    register(&mut author, "qs-author@example.test", "qsauthor").await;
    let mut anon = TestClient::new(build(&config, &db));

    let scored = publish_work(&mut author, "Scored").await;
    let unscored = publish_work(&mut author, "Unscored").await;

    // 0. Nothing is scored yet, and the public read says so with an empty list rather than a fabricated average.
    let (status, body) = anon.get(&format!("/api/v1/works/{scored}/quality")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["signals"].as_array().map(|a| a.len()), Some(0), "{body}");
    assert_eq!(body["composite"], json!(null), "{body}");

    // 1. The door: an operator records a judgement, and the answer is the row that was stored.
    let (status, body) = operator
        .post(
            &format!("/api/v1/admin/works/{scored}/quality-signals"),
            json!({ "kind": "editorial_review", "value": 800, "weight": 1 }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["signal"]["kind"], json!("editorial_review"), "{body}");
    assert_eq!(body["signal"]["value"], json!(800), "{body}");

    // 2. The reader: the signal is visible, with the composite the search filter computes (800 * 1 / 1 = 800 → 0.8).
    let (status, body) = anon.get(&format!("/api/v1/works/{scored}/quality")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["signals"][0]["kind"], json!("editorial_review"), "{body}");
    assert_eq!(body["composite"], json!(0.8), "{body}");

    // 3. The chain that matters: a real search filter now returns the scored work and not the unscored one. Before the door
    //    existed, both were invisible to `min_quality` — the filter answered "nothing" for the whole catalogue.
    let (status, body) = anon.get("/api/v1/media?q=min_quality:750").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let ids: Vec<String> = body["items"]
        .as_array()
        .expect("items")
        .iter()
        .filter_map(|item| item["id"].as_str().map(str::to_owned))
        .collect();
    assert!(ids.contains(&scored), "the scored work must be found: {body}");
    assert!(!ids.contains(&unscored), "an unscored work must not be: {body}");

    // 3b. A correction replaces the operator's own judgement instead of adding a second one. The composite is a weighted mean, so
    //     a duplicate row would double that signal's weight and quietly outvote every other operator's — the value would move to
    //     0.6 for two rows that say 800 and 400, which is not what either operator said.
    let (status, body) = operator
        .post(
            &format!("/api/v1/admin/works/{scored}/quality-signals"),
            json!({ "kind": "editorial_review", "value": 400 }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (status, body) = anon.get(&format!("/api/v1/works/{scored}/quality")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["signals"].as_array().map(|a| a.len()), Some(1), "{body}");
    assert_eq!(body["composite"], json!(0.4), "a correction is not a second opinion: {body}");

    // 4. Refusals by name: the vocabulary and the bounds are the schema's, and a bad request is told which field is wrong.
    let (status, body) = operator
        .post(
            &format!("/api/v1/admin/works/{scored}/quality-signals"),
            json!({ "kind": "vibes", "value": 800 }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["error"]["field_errors"]["kind"], json!("unknown kind"), "{body}");

    let (status, body) = operator
        .post(
            &format!("/api/v1/admin/works/{scored}/quality-signals"),
            json!({ "kind": "editorial_review", "value": 1001 }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["error"]["field_errors"]["value"],
        json!("must be between 0 and 1000"),
        "{body}"
    );

    // 5. The door is an admin door: a writer with no standing gets `404`, not `403` — a refusal would confirm the lock exists.
    let (status, body) = author
        .post(
            &format!("/api/v1/admin/works/{scored}/quality-signals"),
            json!({ "kind": "editorial_review", "value": 900 }),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}
