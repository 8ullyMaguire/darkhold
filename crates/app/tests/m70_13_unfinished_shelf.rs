//! The unfinished shelf: two queries the banner never needed and no page asked for (M70-13 pass 47).
//!
//! `continue_reading` answers "where did I stop, in the one thing I am reading" and it is wired. Beside it live two more
//! queries with no caller:
//!
//! * `unfinished_works` — everything the reader started and has not finished, most recent first. Its own doc says "this is
//!   for the library and for tests", and the library never grew it.
//! * `dnf_works_for` — the works the reader dropped, with the reason, "so the caller can decide" (`abandoned_by_author`
//!   hidden, `triggering` hidden, `not_my_taste` shown again). No caller meant no decision was ever made.
//!
//! A DNF row alone is not a shelf: the reader who dropped ten works wants the ten *titles* and their progress, and a query
//! returning `(work_id, reason)` pairs leaves the client to fetch each work. So the door is one list of started works,
//! annotated with the reason the reader stopped — the judgement the store deliberately left out.
//!
//! # `is_public` is about other readers, not about this one
//!
//! `dnf_works_for` filtered `is_public = 1`, and its doc explained that a private mark "expresses a preference about it
//! that is not 'stop recommending it'". That reading makes `is_public` mean two different things in two places: `GET /me/dnf`
//! — wired, and older than this pass — lists **every** DNF row the reader owns, private ones included. A reader who marked
//! five works privately saw them at `/me/dnf` and in no shelf, and the two lists disagreed about what the reader had done.
//!
//! The column's own name is the tiebreaker: `is_public` says who *else* may see the record. So the shelf shows all of the
//! reader's own marks and carries `public` per row, so the reader can see which of them others can see. The filter is gone,
//! and the reason is written beside the query rather than in this comment alone.

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

/// A published work with one chapter. Returns (work id, unit id).
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
                    { "type": "paragraph", "content": [{ "type": "text", "text": "A chapter to abandon or finish." }] },
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
            json!({ "expected_version": version, "idempotency_key": format!("shelf-{work}") }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    (work, unit)
}

async fn progress(reader: &mut TestClient, work: &str, unit: &str, permille: u16) {
    let (status, body) = reader
        .request(
            "PUT",
            "/api/v1/reading/progress",
            Some(
            json!({
                "subject_type": "work",
                "subject_id": work,
                "unit_id": unit,
                "position_permille": permille,
            }),
            ),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
}

#[tokio::test]
async fn the_shelf_lists_what_i_started_and_why_i_stopped() {
    let dir = scratch_dir("unfinished-shelf");
    let tdb = TestDb::connect_with_dir("m7013-unfinished-shelf", &dir).await;
    let db: Database = tdb.db().clone();
    let config = Config::development_defaults();

    let mut author = TestClient::new(build(&config, &db));
    register(&mut author, "shelf-author@example.test", "shelfauthor").await;
    let mut reader = TestClient::new(build(&config, &db));
    register(&mut reader, "shelf-reader@example.test", "shelfreader").await;
    let mut other = TestClient::new(build(&config, &db));
    register(&mut other, "shelf-other@example.test", "shelfother").await;

    let (kept, kept_unit) = published_work(&mut author, "Still reading").await;
    let (dropped, dropped_unit) = published_work(&mut author, "Dropped").await;

    // 1. Only started works are on the shelf. A work the reader never opened is not "unfinished", it is unread, and a shelf
    //    that showed every work on the instance would be the search page.
    let (status, body) = reader.get("/api/v1/library/unfinished").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["items"], json!([]), "nothing started yet: {body}");

    progress(&mut reader, &kept, &kept_unit, 400).await;
    progress(&mut reader, &dropped, &dropped_unit, 250).await;

    let (status, body) = reader.get("/api/v1/library/unfinished").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let items = body["items"].as_array().expect("items");
    assert_eq!(items.len(), 2, "{body}");
    let titles: Vec<&str> = items
        .iter()
        .filter_map(|i| i["title"].as_str())
        .collect();
    assert!(titles.contains(&"Still reading"), "{body}");
    assert!(titles.contains(&"Dropped"), "{body}");
    // The percentage is the store's rounding, surfaced — a client that only wants a progress bar should not have to
    // re-derive it, and the unrounded permille is beside it for one that wants "page 12 of 340".
    let kept_row = items
        .iter()
        .find(|i| i["title"] == json!("Still reading"))
        .expect("kept row");
    assert_eq!(kept_row["position_permille"], json!(400), "{body}");
    assert_eq!(kept_row["percent"], json!(40), "{body}");
    assert_eq!(kept_row["dnf_reason"], serde_json::Value::Null, "{body}");

    // 2. A dropped work stays on the shelf *with the reason*, which is the judgement the store left to the caller. It does
    //    not disappear: a reader who stops reading a thing has not stopped having started it, and the reason is what lets
    //    the client decide whether to show it again.
    let (status, body) = reader
        .post(
            &format!("/api/v1/works/{dropped}/dnf"),
            json!({ "reason": "slow_pacing", "note": "too slow", "is_public": false }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = reader.get("/api/v1/library/unfinished").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let items = body["items"].as_array().expect("items");
    assert_eq!(items.len(), 2, "dropping one does not remove it from the shelf: {body}");
    let dropped_row = items
        .iter()
        .find(|i| i["title"] == json!("Dropped"))
        .expect("dropped row");
    assert_eq!(dropped_row["dnf_reason"], json!("slow_pacing"), "{body}");
    // The reader's own private mark is visible to the reader, and the row says it is private. `is_public` is about other
    // readers; `GET /me/dnf` has always listed private rows, and a shelf that hid them would contradict it.
    assert_eq!(dropped_row["dnf_public"], json!(false), "{body}");
    assert_eq!(
        dropped_row["dnf_note"],
        json!("too slow"),
        "the reader sees their own note: {body}"
    );

    // 3. Somebody else's shelf is empty, not somebody else's shelf. A reader's progress is not a shared fact.
    let (status, body) = other.get("/api/v1/library/unfinished").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["items"], json!([]), "{body}");

    // 4. The limit is a limit, and it is required rather than trusted: `LIMIT` with an unbounded value from a query string
    //    is an unbounded result set. Out-of-range values are clamped by the store, not refused.
    let (status, body) = reader.get("/api/v1/library/unfinished?limit=1").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["items"].as_array().expect("items").len(), 1, "{body}");
    let (status, body) = reader.get("/api/v1/library/unfinished?limit=10000").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["limit"], json!(50),
        "the ceiling is reported so a client can page honestly: {body}"
    );

    // 5. A DNF with no progress row is not on the shelf — the shelf lists started works, and the DNF-only list already
    //    exists at `/me/dnf`. This is the boundary between the two doors, asserted so neither drifts into the other.
    let (unstarted, _) = published_work(&mut author, "Never opened").await;
    let (status, body) = reader
        .post(
            &format!("/api/v1/works/{unstarted}/dnf"),
            json!({ "reason": "not_my_taste", "is_public": false }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = reader.get("/api/v1/library/unfinished").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["items"].as_array().expect("items").len(),
        2,
        "a DNF without progress is not an unfinished work: {body}"
    );

    // 6. The shelf is a signed-in reader's own page; there is no anonymous version of "what did you start".
    let mut anonymous = TestClient::new(build(&config, &db));
    let (status, body) = anonymous.get("/api/v1/library/unfinished").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
}
