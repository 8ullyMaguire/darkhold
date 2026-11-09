//! A tag's confidence, and the reader who is the only thing that can move it (M70-13 pass 63).
//!
//! `taxonomy::recompute_confidence` maintained `work_tags.confidence` from `work_tag_votes` and had no caller — but the reason it
//! had no caller was not that nobody had wired it: **nothing in the binary wrote a vote and nothing read `work_tags.confidence`.**
//! No `wt.confidence` appeared in a single query. Wiring the sweep would have made a number fresher that no surface showed and no
//! product path fed, which is ceremony around a dead column.
//!
//! So the pass is the missing half, not the sweep: §6.4's reader layer. A reader says a tag is right (`1`), wrong (`-1`), or
//! takes the claim back (`0`), the confidence for that one (work, tag) is recomputed in the same request, and the reader can see
//! their own vote beside it. The whole-table sweep is then what it always was — the repair tool for drift — and it gets an
//! operator door, because a sweep nobody can run is how the column drifted in the first place.
//!
//! Four decisions:
//!
//! * **A vote is pseud-scoped and one row per (work, tag, pseud).** The primary key says so; a reader who votes twice is one
//!   opinion, not two votes.
//! * **`0` withdraws the row instead of storing an abstention.** "I take it back" is not evidence against the tag, and counting it
//!   in the denominator would let four readers who all withdrew pull a tag's confidence down — a disagreement count with no
//!   disagreement in it.
//! * **A tag the work does not carry answers `404`.** There is no claim to judge; `422` would say the request was malformed, and
//!   the request was fine.
//! * **The vote is visible only to the person who cast it.** `GET` answers with *your* vote and the public confidence; it never
//!   says how anybody else voted, because a scoped opinion published with a handle is a durable public judgement about another
//!   person's work.

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

async fn a_work(author: &mut TestClient, title: &str) -> String {
    let (status, body) = author.post("/api/v1/works", json!({ "title": title })).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_str().expect("work id").to_owned()
}

async fn a_node(author: &mut TestClient, canonical: &str) -> String {
    let (status, body) = author
        .post("/api/v1/taxonomy", json!({ "kind": "tag", "canonical": canonical }))
        .await;
    assert!(status.is_success(), "{body}");
    body["node"]["id"].as_str().expect("node id").to_owned()
}

#[tokio::test]
async fn a_reader_can_say_a_tag_is_right_and_the_confidence_moves() {
    let dir = scratch_dir("tag-votes");
    let tdb = TestDb::connect_with_dir("m7013-tag-votes", &dir).await;
    let db: Database = tdb.db().clone();
    let config = Config::development_defaults();

    let mut author = TestClient::new(build(&config, &db));
    register(&mut author, "tv-author@example.test", "tvauthor").await;
    let work = a_work(&mut author, "Tagged").await;
    let node = a_node(&mut author, "angst").await;
    let (status, body) = author
        .post(
            &format!("/api/v1/works/{work}/tags"),
            json!({ "node_id": node }),
        )
        .await;
    assert!(status.is_success(), "the work carries the tag: {body}");

    let mut a = TestClient::new(build(&config, &db));
    register(&mut a, "tv-a@example.test", "readera").await;
    let mut b = TestClient::new(build(&config, &db));
    register(&mut b, "tv-b@example.test", "readerb").await;

    let vote = format!("/api/v1/works/{work}/tags/{node}/vote");

    // 1. Nothing has been said yet, and the answer says so rather than `null`-as-an-error. With no votes §6.4's smoothing gives
    //    1.0 — a claim nobody has disputed is not a claim nobody believes.
    let (status, body) = a.get(&vote).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["vote"], json!(null), "{body}");
    assert!(
        (body["confidence"].as_f64().expect("confidence") - 1.0).abs() < 1e-9,
        "an unopposed tag reads as 1.0: {body}"
    );

    // 2. A reader agrees.
    let (status, body) = a.request("PUT", &vote, Some(json!({ "vote": 1 }))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["vote"], json!(1), "{body}");

    // 3. A second reader disagrees, and the number moves for the first reader too — it is a property of the tag, not of the
    //    request that asked. Two votes, one of them positive: (1 + 1) / (2 + 1).
    let (status, body) = b.request("PUT", &vote, Some(json!({ "vote": -1 }))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let disputed = body["confidence"].as_f64().expect("confidence");
    assert!(
        (disputed - 2.0 / 3.0).abs() < 1e-6,
        "one agreement and one disagreement on a tag whose claim is already smoothed: {body}"
    );

    // 4. And the first reader still sees exactly their own vote. Publishing how a named person voted would turn a scoped opinion
    //    into a durable public judgement about somebody else's work.
    let (status, body) = a.get(&vote).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["vote"],
        json!(1),
        "the other reader's `-1` must not appear in A's answer: {body}"
    );
    assert!(
        (body["confidence"].as_f64().expect("confidence") - disputed).abs() < 1e-9,
        "the confidence is shared: {body}"
    );

    // 5. `0` withdraws the row. "I take it back" is not evidence against the tag, so it leaves the denominator: one vote, and it
    //    agrees — (1 + 1) / (1 + 1).
    let (status, body) = a.request("PUT", &vote, Some(json!({ "vote": 0 }))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["vote"], json!(null), "a withdrawn vote is no vote: {body}");
    let (_, body) = a.get(&vote).await;
    assert_eq!(body["vote"], json!(null), "{body}");
    assert!(
        (body["confidence"].as_f64().expect("confidence") - 0.5).abs() < 1e-9,
        "the withdrawn vote left the denominator rather than counting as disagreement: {body}"
    );
    // Counted per engine: a `sqlite_pool().expect("sqlite")` in a fixture makes the whole file SQLite-only, which is how a
    // PostgreSQL defect survives a green suite.
    let rows: i64 = match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar(
            "SELECT COUNT(*) FROM work_tag_votes WHERE work_id = ? AND node_id = ?",
        )
        .bind(&work)
        .bind(&node)
        .fetch_one(db.sqlite_pool().expect("sqlite"))
        .await
        .expect("vote rows"),
        darkhold_db::Backend::Postgres => sqlx::query_scalar(
            "SELECT COUNT(*) FROM work_tag_votes WHERE work_id::text = $1 AND node_id = $2",
        )
        .bind(&work)
        .bind(&node)
        .fetch_one(db.postgres_pool().expect("postgres"))
        .await
        .expect("vote rows"),
    };
    assert_eq!(rows, 1, "only B's row is left");

    // 6. A tag the work does not carry has no claim to judge: `404`, not `422`. The request was well formed.
    let other_node = a_node(&mut author, "fluff").await;
    let (status, _) = b
        .request(
            "PUT",
            &format!("/api/v1/works/{work}/tags/{other_node}/vote"),
            Some(json!({ "vote": 1 })),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "voting on a tag the work does not carry"
    );

    //    The read door answers the same way for the same reason: `1.0` would say "nobody has disputed this tag on this work", and
    //    there is no tag on this work to dispute.
    let (status, _) = a
        .get(&format!("/api/v1/works/{work}/tags/{other_node}/vote"))
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "reading a vote about a tag the work does not carry"
    );

    // 7. A vote outside the vocabulary is refused by name, so a client is told which field it got wrong.
    let (status, body) = a.request("PUT", &vote, Some(json!({ "vote": 5 }))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body.to_string().contains("vote"),
        "the refusal names the vote field: {body}"
    );

    // 8. An operator can run the repair pass, which is the whole-table sweep that used to be the only half of this feature: it
    //    recomputes from the rows that now exist, and reports how many tags it touched.
    let mut operator = TestClient::new(build(&config, &db));
    let account = register(&mut operator, "tv-op@example.test", "tvoperator").await;
    let mut admin = Config::development_defaults();
    admin.administration.operator_account_id = Some(account.parse().expect("account id"));
    let mut operator = TestClient::new(build(&admin, &db));
    test_support::sign_in_as(&mut operator, &tdb, "tv-op@example.test", "tvoperator").await;

    let (status, body) = operator
        .post("/api/v1/admin/taxonomy/confidence/recompute", json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["updated"].as_i64().expect("updated") >= 1,
        "the repair says how many tags it recomputed: {body}"
    );

    // The repair agrees with what the vote path already stored: a sweep that changes the number a reader just saw would mean the
    // two were computing different things.
    let (_, body) = a.get(&vote).await;
    assert!(
        (body["confidence"].as_f64().expect("confidence") - 0.5).abs() < 1e-9,
        "the sweep and the vote path compute the same confidence: {body}"
    );

    // 9. No session, no vote: a reader's opinion needs a reader to hold it.
    let mut anon = TestClient::new(build(&config, &db));
    let (status, _) = anon.request("PUT", &vote, Some(json!({ "vote": 1 }))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    tdb.cleanup().await;
}
