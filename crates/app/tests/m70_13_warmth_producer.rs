//! The audience panel read a table nothing wrote (M70-13).
//!
//! §41.2 defines warmth as a reader→author score with tiers, and the code has every part of the rule: `warmth_delta` gives the
//! basis points per action (ReadUnit 100, FinishWork 500, React 200, Comment 400), `tier_for` maps a score to a tier, and
//! `GET /me/audience` aggregates the tiers for an author.
//!
//! The one thing missing was the writer. `longevity::record_warmth` had no caller anywhere in `crates/app`, and the deployed
//! database measures the consequence: **`interaction_warmth` holds 0 rows**. So every author's audience panel answered
//! `{lurk: 0, react: 0, comment: 0, create: 0, total: 0}` — a panel that cannot tell "nobody reads me" from "nothing is recorded",
//! on an instance with 58,042 works and readers who kudos and comment every day.
//!
//! This test drives the panel through the two acts that are one call each — a kudos and a comment — and asserts the tiers move:
//!
//! * a kudos is `React` (200 bp), so the reader appears in the `react` tier at once (threshold 200);
//! * three comments are 3 × 400 = 1200 bp, which crosses the `comment` threshold (1000), so the same reader moves up a tier —
//!   that is the accumulation the additive upsert exists for, and a per-act overwrite would fail it;
//! * **a reader cannot warm themselves**: an author who kudos their own work must not appear in their own panel, or the panel's
//!   `create` tier would count the one person guaranteed to be there;
//! * **removing a kudos removes the warmth**: the toggle is symmetric, and a reader who tries the button twice must not end up
//!   warmer than one who pressed it once.

use axum::http::StatusCode;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::Database;
use serde_json::json;
use test_support::{register, scratch_dir, TestClient, TestDb};

fn build(config: &Config, db: &Database) -> axum::Router {
    server::build_router(AppState::new(config.clone(), db.clone()))
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
                        "content": [{ "type": "text", "text": "Some words a reader can open." }]
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

async fn audience(author: &mut TestClient) -> serde_json::Value {
    let (status, body) = author.get("/api/v1/me/audience").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

#[tokio::test]
async fn a_kudos_and_a_comment_move_the_authors_audience_panel() {
    let dir = scratch_dir("warmth-producer");
    let tdb = TestDb::connect_with_dir("m7013-warmth-producer", &dir).await;
    let db: Database = tdb.db().clone();
    let config = Config::development_defaults();

    let mut author = TestClient::new(build(&config, &db));
    register(&mut author, "wm-author@example.test", "wmauthor").await;

    let mut reader = TestClient::new(build(&config, &db));
    register(&mut reader, "wm-reader@example.test", "wmreader").await;

    let work = publish_work(&mut author, "Warmth").await;

    // 0. The panel starts empty — and that is the only honest reading of the number, which is the whole complaint: an empty panel
    //    said the same thing as a broken producer.
    assert_eq!(audience(&mut author).await["total"], json!(0));

    // 1. A kudos is `React` (200 bp), which is exactly the `react` threshold.
    let (status, body) = reader
        .request(
            "POST",
            &format!("/api/v1/works/{work}/kudos"),
            Some(json!({})),
        )
        .await;
    assert!(status.is_success(), "kudos: {body}");
    let panel = audience(&mut author).await;
    assert_eq!(panel["react"], json!(1), "{panel}");
    assert_eq!(panel["total"], json!(1), "{panel}");

    // 2. Removing the kudos removes the warmth: the toggle is symmetric, so pressing it twice cannot leave a reader warmer than
    //    pressing it once.
    let (status, body) = reader
        .request(
            "POST",
            &format!("/api/v1/works/{work}/kudos"),
            Some(json!({})),
        )
        .await;
    assert!(status.is_success(), "un-kudos: {body}");
    assert_eq!(audience(&mut author).await["total"], json!(0));

    // The discussion mode defaults to ThreadOnly (§35.0), so the author opens comments on this work first — the same switch a real
    // author flips, rather than a test-only fixture.
    let (status, body) = author
        .request(
            "PUT",
            &format!("/api/v1/works/{work}/discussion-mode"),
            Some(json!({ "mode": "comments_only" })),
        )
        .await;
    assert!(status.is_success(), "discussion mode: {body}");

    // 3. Three comments are 3 × 400 = 1200 bp, past the `comment` threshold (1000): the same reader moves up a tier, which is the
    //    accumulation the additive upsert exists for.
    for index in 0..3 {
        let (status, body) = reader
            .post(
                &format!("/api/v1/works/{work}/comments"),
                json!({ "body": format!("comment {index}") }),
            )
            .await;
        assert!(status.is_success(), "comment: {body}");
    }
    let panel = audience(&mut author).await;
    assert_eq!(panel["comment"], json!(1), "{panel}");
    assert_eq!(panel["total"], json!(1), "one reader, one relationship: {panel}");

    // 4. A reader cannot warm themselves: the author commenting on their own work must not appear in their own panel.
    let (status, body) = author
        .post(
            &format!("/api/v1/works/{work}/comments"),
            json!({ "body": "author note" }),
        )
        .await;
    assert!(status.is_success(), "author comment: {body}");
    let panel = audience(&mut author).await;
    assert_eq!(
        panel["total"],
        json!(1),
        "the author must not count as their own reader: {panel}"
    );
    assert_eq!(panel["comment"], json!(1), "{panel}");
}
