//! How many people read this work — the capability that said `not_implemented` while its query existed (M70-13 pass 50).
//!
//! `GET /me/analytics/{capability}` derives `implemented` from whether a query ran, and answers
//! `{"status": "not_implemented"}` for every scope without one. `own.work.basic` — reader count, word count, unit count for a
//! work you wrote — was in that list, while `analytics::work_reader_count`, the query written for it, had no caller at all.
//! So the dashboard told an author the instance could not answer a question it had already implemented, and the author
//! looking at one of their works had no number: the dashboard's totals are across all their works.
//!
//! The number lives on the authoring view of a work, behind `Scope::OwnWorkBasic`, which is trust level 0 — every author may
//! ask about their own work — and after the store's floor of **ten distinct pseudonyms**. Below ten the answer is
//! `fewer_than: 10` with no `count`: a band, not a clamp, because a zero would be a claim and a suppressed number is not
//! "nobody read it".

use axum::http::StatusCode;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::Database;
use serde_json::{json, Value};
use test_support::{register, scratch_dir, TestClient, TestDb};

fn build(config: &Config, db: &Database) -> axum::Router {
    server::build_router(AppState::new(config.clone(), db.clone()))
}

/// A published work with one chapter: (work id, unit id).
async fn published_work(author: &mut TestClient, title: &str) -> (String, String) {
    let (status, body) = author.post("/api/v1/works", json!({ "title": title })).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let work = body["id"].as_str().expect("work").to_owned();
    let (status, body) = author
        .post(
            &format!("/api/v1/works/{work}/chapters"),
            json!({ "title": "One" }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let unit = body["id"].as_str().expect("unit").to_owned();
    let unit_version = body["version"].as_i64().expect("unit version");
    let (status, body) = author
        .request(
            "PATCH",
            &format!("/api/v1/chapters/{unit}"),
            Some(json!({
                "expected_version": unit_version,
                "document": { "type": "doc", "content": [
                    { "type": "paragraph", "content": [{ "type": "text", "text": "Someone reads this." }] },
                ]},
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, work_body) = author.get(format!("/api/v1/works/{work}")).await;
    let version = work_body["version"].as_i64().expect("work version");
    let (status, body) = author
        .post(
            &format!("/api/v1/works/{work}/publish"),
            json!({ "expected_version": version, "idempotency_key": format!("aud-{work}") }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    (work, unit)
}

/// One reader opening the work. `PUT /reading/progress` is what writes `reading_history_entry`, which is what the count
/// reads — a reader who only fetched the JSON has not been counted, and that is the store's decision, not this test's.
async fn read_once(reader: &mut TestClient, work: &str, unit: &str) {
    let (status, body) = reader
        .request(
            "PUT",
            "/api/v1/reading/progress",
            Some(json!({
                "subject_type": "work",
                "subject_id": work,
                "unit_id": unit,
                "position_permille": 120,
            })),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
}

#[tokio::test]
async fn an_author_sees_a_banded_reader_count_and_nobody_else_does() {
    let dir = scratch_dir("work-audience");
    let tdb = TestDb::connect_with_dir("m7013-work-audience", &dir).await;
    let db: Database = tdb.db().clone();
    let config = Config::development_defaults();

    let mut author = TestClient::new(build(&config, &db));
    register(&mut author, "aud-author@example.test", "audauthor").await;
    let mut reader = TestClient::new(build(&config, &db));
    register(&mut reader, "aud-1@example.test", "audreader1").await;

    let (work, unit) = published_work(&mut author, "Read by some").await;

    // 1. One reader, and the answer is a band: `fewer_than` and no `count`. A zero here would be "nobody read it", which is
    //    not what the store knows — it knows the number is under the floor.
    read_once(&mut reader, &work, &unit).await;
    let (status, body) = author.get(format!("/api/v1/works/{work}")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let analytics = &body["analytics"];
    assert_eq!(analytics["status"], json!("ok"), "{body}");
    let readers = &analytics["readers"];
    assert_eq!(readers["fewer_than"], json!(10), "{body}");
    assert_eq!(readers["count"], Value::Null, "a banded count carries no count: {body}");
    assert_eq!(readers["scope"], json!("own.work.basic"), "{body}");
    // The method travels with the number, because a number with no definition is a number nobody can check.
    assert!(
        readers["method"]["definition"].as_str().is_some_and(|m| m.contains("pseudonym")),
        "the method says what a reader is: {body}"
    );

    // 2. At the floor the number becomes exact. Nine more readers, ten in all, then the count appears and the band goes.
    for i in 2..=10 {
        let mut more = TestClient::new(build(&config, &db));
        register(&mut more, &format!("aud-{i}@example.test"), &format!("audreader{i}")).await;
        read_once(&mut more, &work, &unit).await;
    }
    let (status, body) = author.get(format!("/api/v1/works/{work}")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let readers = &body["analytics"]["readers"];
    assert_eq!(readers["count"], json!(10), "{body}");
    assert_eq!(
        readers["fewer_than"],
        Value::Null,
        "at the floor the band is replaced by the number: {body}"
    );

    // 3. The same reader opening it again is one reader. `COUNT(DISTINCT pseud)` is the store's rule and the door must not
    //    inflate it — a work that counts events invents an audience.
    read_once(&mut reader, &work, &unit).await;
    let (status, body) = author.get(format!("/api/v1/works/{work}")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["analytics"]["readers"]["count"], json!(10), "{body}");

    // 4. A signed-in stranger sees the public view and no analytics at all — not a zero, not an empty object. The key is
    //    absent because the author's numbers are not a fact about the work.
    let mut stranger = TestClient::new(build(&config, &db));
    register(&mut stranger, "aud-stranger@example.test", "audstranger").await;
    let (status, body) = stranger.get(format!("/api/v1/works/{work}")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body.get("analytics"),
        None,
        "a reader's view of a work carries no author analytics: {body}"
    );

    // 5. Anonymous, likewise.
    let mut anonymous = TestClient::new(build(&config, &db));
    let (status, body) = anonymous.get(format!("/api/v1/works/{work}")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body.get("analytics"), None, "{body}");
}
