//! A work's publication history, which the table recorded and no page showed (M70-13).
//!
//! `publication_events` is written on every publish, update and revision restore. It was read by one listing's sort key and
//! nowhere else, so the two facts a reader wants about a work — *when was this published* and *when did it last change* —
//! were recorded and never shown. A work finished in 2019 and one finished yesterday rendered identically.
//!
//! The door answers both maxima (`work_freshness`) and the evidence behind them (`publication_events`), so "updated 3 days
//! ago" can be clicked into and a restored revision is distinguishable from a fresh edit.
//!
//! Visibility is `read_work`'s decision, re-used rather than re-implemented: the same `reading_decision_publishing_metadata`
//! call decides whether the caller may see the work, and a denied caller gets exactly what reading the work would give them.
//! That matters here more than usual — the "does this work exist" question must not have two answers depending on which door
//! you ask, or a history endpoint becomes an oracle for the existence of drafts.

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

#[tokio::test]
async fn the_record_shows_when_it_was_published_and_when_it_last_changed() {
    let dir = scratch_dir("work-history");
    let tdb = TestDb::connect_with_dir("m7013-work-history", &dir).await;
    let db: Database = tdb.db().clone();
    let config = Config::development_defaults();

    let mut author = TestClient::new(build(&config, &db));
    register(&mut author, "wh-author@example.test", "whauthor").await;
    let mut stranger = TestClient::new(build(&config, &db));
    register(&mut stranger, "wh-stranger@example.test", "whstranger").await;

    // 1. A draft has never been published: `state` says so rather than leaving a client to interpret a null.
    let (status, body) = author.post("/api/v1/works", json!({ "title": "Draft" })).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let draft = body["id"].as_str().expect("work").to_owned();
    let (status, history) = author.get(format!("/api/v1/works/{draft}/history")).await;
    assert_eq!(status, StatusCode::OK, "{history}");
    assert_eq!(history["state"], json!("never_published"), "{history}");
    assert_eq!(history["published_at"], Value::Null, "{history}");
    assert_eq!(history["updated_at"], Value::Null, "{history}");
    assert_eq!(history["events"], json!([]), "{history}");

    // 2. Somebody else's draft is not merely history-less, it is absent — the same answer reading it gives.
    let (status, body) = stranger.get(format!("/api/v1/works/{draft}/history")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // Publish something with text, else the publication is refused ("an empty work has nothing to read").
    let (status, body) = author
        .post(
            &format!("/api/v1/works/{draft}/chapters"),
            json!({ "title": "Chapter One" }),
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
                    { "type": "paragraph", "content": [{ "type": "text", "text": "The first chapter of a story." }] },
                ]},
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, work) = author.get(format!("/api/v1/works/{draft}")).await;
    let version = work["version"].as_i64().expect("work version");
    let (status, body) = author
        .post(
            &format!("/api/v1/works/{draft}/publish"),
            json!({ "expected_version": version, "idempotency_key": format!("wh-{draft}") }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // 3. After publication the record carries the date, and the event behind it.
    let (status, history) = author.get(format!("/api/v1/works/{draft}/history")).await;
    assert_eq!(status, StatusCode::OK, "{history}");
    assert_eq!(history["state"], json!("published"), "{history}");
    let published_at = history["published_at"].as_str().expect("published_at").to_owned();
    assert!(!published_at.is_empty(), "{history}");
    let events = history["events"].as_array().expect("events");
    assert_eq!(events.len(), 1, "{history}");
    assert_eq!(events[0]["action"], json!("publish"), "{history}");
    let (_, work) = author.get(format!("/api/v1/works/{draft}")).await;
    let version_after_publish = work["version"].as_i64().expect("version after publish");

    // 4. The history of a published work is public: an anonymous reader is told when it was published.
    let mut anonymous = TestClient::new(build(&config, &db));
    let (status, history) = anonymous.get(format!("/api/v1/works/{draft}/history")).await;
    assert_eq!(status, StatusCode::OK, "{history}");
    assert_eq!(history["published_at"].as_str(), Some(published_at.as_str()));
    let _ = &mut anonymous;

    // 5. An edit moves `updated_at` and leaves `published_at` alone — the distinction the two maxima exist for. The events
    //    list is newest-first, so the edit is at the head. The pause is not decoration: `occurred_at` is second-resolution, and
    //    a publish and an edit in the same second tie, which would make "newest first" an assertion the data cannot support
    //    (the first version of this test failed exactly there, on PostgreSQL, where the tie broke publish-first).
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let (status, body) = author
        .request(
            "PATCH",
            &format!("/api/v1/works/{draft}"),
            Some(json!({ "expected_version": version_after_publish, "title": "Draft, revised" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, history) = author.get(format!("/api/v1/works/{draft}/history")).await;
    assert_eq!(status, StatusCode::OK, "{history}");
    assert_eq!(
        history["published_at"].as_str(),
        Some(published_at.as_str()),
        "an edit is not a republication: {history}"
    );
    let updated_at = history["updated_at"].as_str().expect("updated_at").to_owned();
    assert!(
        updated_at > published_at,
        "the update is strictly after the publication, a second later: {history}"
    );
    let events = history["events"].as_array().expect("events");
    assert!(events.len() >= 2, "the edit is recorded: {history}");
    assert_eq!(events[0]["action"], json!("update"), "{history}");
    assert_eq!(events[1]["action"], json!("publish"), "{history}");

    // 6. The limit is a limit.
    let (status, history) = author
        .get(format!("/api/v1/works/{draft}/history?limit=1"))
        .await;
    assert_eq!(status, StatusCode::OK, "{history}");
    assert_eq!(history["events"].as_array().expect("events").len(), 1, "{history}");
}
