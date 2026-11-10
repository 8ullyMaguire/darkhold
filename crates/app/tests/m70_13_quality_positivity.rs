//! `reader_positivity`: the one quality signal the corpus can prove (M70-13).
//!
//! Pass 80 shipped the door an operator uses, and named the two kinds a pass could derive. `completeness` turned out not to be one of
//! them: `completion:complete` is already a filter, so a derived signal would be a second way to say the same thing with a number
//! attached — an invented curve over a stored enum.
//!
//! `reader_positivity` is arithmetic on something that already exists: the classifier in `darkhold_domain::positivity` reads review
//! text and returns a class. The signal is `round(1000 * positive / visible)` over the reviews a reader can see — the same list
//! `positivity::visible_reviews` serves — and it is recomputed on every review write, so it describes the page rather than the
//! history of the page.
//!
//! What this test pins, in the order the numbers move:
//!
//! * **no reviews is not zero positivity**: the signal does not exist, and the composite is `null`. A `0.0` here would sort below
//!   every scored work as though somebody had judged it unkindly.
//! * one positive review is `1000` (one of one);
//! * a second, hostile review halves it — **recomputed, not accumulated**: an incremental average would keep the first reader's
//!   verdict winning forever;
//! * deleting the positive review leaves `0` for one hostile review, which is a measurement rather than an absence.

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
    let (status, work) = author
        .post("/api/v1/works", json!({ "title": title }))
        .await;
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

async fn positivity(anon: &mut TestClient, work: &str) -> (Option<i64>, serde_json::Value) {
    let (status, body) = anon.get(&format!("/api/v1/works/{work}/quality")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let value = body["signals"]
        .as_array()
        .expect("signals")
        .iter()
        .find(|signal| signal["kind"] == json!("reader_positivity"))
        .and_then(|signal| signal["value"].as_i64());
    (value, body["composite"].clone())
}

#[tokio::test]
async fn a_review_makes_reader_positivity_real_and_recomputes_on_each_write() {
    let dir = scratch_dir("quality-positivity");
    let tdb = TestDb::connect_with_dir("m7013-quality-positivity", &dir).await;
    let db: Database = tdb.db().clone();
    let mut config = Config::development_defaults();
    config.storage.root = dir.clone();

    let mut author = TestClient::new(build(&config, &db));
    register(&mut author, "qp-author@example.test", "qpauthor").await;
    let mut kind = TestClient::new(build(&config, &db));
    register(&mut kind, "qp-kind@example.test", "qpkind").await;
    let mut critic = TestClient::new(build(&config, &db));
    register(&mut critic, "qp-critic@example.test", "qpcritic").await;
    let mut anon = TestClient::new(build(&config, &db));

    let work = publish_work(&mut author, "Reviewed").await;

    // 0. No reviews: the signal must not exist. `0` would claim a verdict nobody reached.
    let (value, composite) = positivity(&mut anon, &work).await;
    assert_eq!(
        value, None,
        "unreviewed work must carry no positivity signal"
    );
    assert_eq!(composite, json!(null));

    // 1. One positive review is one of one.
    let (status, body) = kind
        .request(
            "PUT",
            &format!("/api/v1/works/{work}/reviews"),
            Some(json!({
                "body": "I loved this, wonderful and warm throughout.",
                "is_public": true
            })),
        )
        .await;
    assert!(status.is_success(), "positive review: {body}");
    let (value, _) = positivity(&mut anon, &work).await;
    assert_eq!(value, Some(1000), "one positive review of one");

    // 2. A hostile review is held by the gate (§12) and a held review is not on the page, so the number does not move. That is the
    //    property worth pinning: the signal follows what readers can see, not what the database stores.
    let (status, body) = critic
        .request(
            "PUT",
            &format!("/api/v1/works/{work}/reviews"),
            Some(json!({
                "body": "You are an idiot and this is worthless.",
                "is_public": true
            })),
        )
        .await;
    assert!(status.is_success(), "hostile review: {body}");
    let (value, _) = positivity(&mut anon, &work).await;
    assert_eq!(value, Some(1000), "a held review is not on the page");
    let (status, list) = anon.get(&format!("/api/v1/works/{work}/reviews")).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(
        list["items"].as_array().map(|reviews| reviews.len()),
        Some(1),
        "the held review must not be listed: {list}"
    );

    // 2b. The generic verdict the classifier used to call praise: two dislike markers, no craft-specificity, no personal attack. It
    //     is held by the gate now, so it is neither delivered to the author nor counted here. Before pass 82 it was `Positive`, which
    //     meant an author received "this is awful and boring" as a review and the quality signal called it praise.
    let mut disliker = TestClient::new(build(&config, &db));
    register(&mut disliker, "qp-disliker@example.test", "qpdisliker").await;
    let (status, body) = disliker
        .request(
            "PUT",
            &format!("/api/v1/works/{work}/reviews"),
            Some(json!({
                "body": "This is awful and boring. I hate it.",
                "is_public": true
            })),
        )
        .await;
    assert!(status.is_success(), "generic dislike: {body}");
    let (value, _) = positivity(&mut anon, &work).await;
    assert_eq!(
        value,
        Some(1000),
        "a held generic verdict must not count as praise"
    );

    // 3. A visible review that is *not* positive counts in the denominator. The author opts in to constructive critique, the critique
    //    is delivered by the next edit, and the value halves — so "positive" is doing real work rather than being the default label
    //    for every visible row.
    let (status, body) = author
        .request(
            "PUT",
            "/api/v1/feedback/preferences",
            Some(json!({ "accept_constructive": true })),
        )
        .await;
    assert!(status.is_success(), "preferences: {body}");
    let (status, body) = critic
        .request(
            "PUT",
            &format!("/api/v1/works/{work}/reviews"),
            Some(json!({
                "body": "The pacing drags in the middle chapters.",
                "is_public": true
            })),
        )
        .await;
    assert!(status.is_success(), "critique: {body}");
    let (value, _) = positivity(&mut anon, &work).await;
    assert_eq!(
        value,
        Some(500),
        "one positive of two visible: recomputed, not accumulated"
    );

    // 4. Deleting the positive review leaves one visible critique: `0`, not absent, and not the stale `1000`. A visible row that is
    //    not positive must count as not-positive — this is the assertion that fails if every visible review is treated as praise.
    let (status, body) = kind
        .request("DELETE", &format!("/api/v1/works/{work}/reviews"), None)
        .await;
    assert!(status.is_success(), "delete review: {body}");
    let (value, _) = positivity(&mut anon, &work).await;
    assert_eq!(value, Some(0), "no positive reviews of one visible");

    // 5. Deleting the last visible review leaves nothing visible, so the signal is **absent** — not `0`, and not the stale `0` from
    //    the previous step. An incremental writer would still be reporting a number here; that is what recomputation buys.
    let (status, body) = critic
        .request("DELETE", &format!("/api/v1/works/{work}/reviews"), None)
        .await;
    assert!(status.is_success(), "delete critique: {body}");
    let (value, composite) = positivity(&mut anon, &work).await;
    assert_eq!(
        value, None,
        "nothing visible means no signal, and no stale one"
    );
    assert_eq!(composite, json!(null));
}
