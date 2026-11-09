//! "Is this in my queue?" — the question the module refused and no other surface answered (M70-13 pass 48).
//!
//! `marked_for_later` had three wired doors (`POST`/`DELETE /works/{id}/marked-for-later`, `GET /me/marked-for-later`) and
//! one orphan: `is_marked(db, account, work) -> bool`.
//!
//! The module comment justified the orphan by naming the mechanism that was supposed to answer it instead:
//!
//! > Asking "is this work in *my* queue" is answered by `WORK_STATE` on the work page, and a per-work read would invite a
//! > caller to ask about somebody else's queue with the same URL shape.
//!
//! `WORK_STATE` does not exist — not in the Rust tree, not in the frontend. The comment cites a design that was never built,
//! and the shape it refuses (a per-work read) is the only practical way for a reader looking at a work to see whether the
//! bookmark button on it is already filled. Without it a client must fetch the whole queue and search it, which is what a
//! reader with 200 saved works would be paying for on every work page.
//!
//! # Scoped to the caller, and the URL says so
//!
//! The comment's real objection was not safety — a door behind `RequireSession` can only read the caller's own queue — but
//! that `/works/{id}/...` reads as a fact about the work rather than about the caller. So the read lives at
//! `GET /me/marked-for-later/{work}`: the same `/me/` scoping as the queue it belongs to, and the same answer in one call.

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
    body["id"].as_str().expect("work").to_owned()
}

#[tokio::test]
async fn a_reader_can_ask_whether_this_work_is_in_their_queue() {
    let dir = scratch_dir("marked-state");
    let tdb = TestDb::connect_with_dir("m7013-marked-state", &dir).await;
    let db: Database = tdb.db().clone();
    let config = Config::development_defaults();

    let mut author = TestClient::new(build(&config, &db));
    register(&mut author, "ms-author@example.test", "msauthor").await;
    let mut reader = TestClient::new(build(&config, &db));
    register(&mut reader, "ms-reader@example.test", "msreader").await;
    let mut other = TestClient::new(build(&config, &db));
    register(&mut other, "ms-other@example.test", "msother").await;

    let work = a_work(&mut author, "Queued or not").await;

    // 1. Not saved: a complete answer, not a 404. The question is about the reader's list, and "no" answers it.
    let (status, body) = reader
        .get(format!("/api/v1/me/marked-for-later/{work}"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["marked"], json!(false), "{body}");
    assert_eq!(body["work_id"], json!(work), "{body}");

    // 2. Queued, and the page can now fill the button without fetching the queue.
    let (status, body) = reader
        .post(&format!("/api/v1/works/{work}/marked-for-later"), json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["added"], json!(true), "{body}");
    let (status, body) = reader
        .get(format!("/api/v1/me/marked-for-later/{work}"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["marked"], json!(true), "{body}");

    // 3. Queueing twice is not two entries: the write door is idempotent and the read agrees with it.
    let (status, body) = reader
        .post(&format!("/api/v1/works/{work}/marked-for-later"), json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["added"], json!(false), "second queue is a no-op: {body}");

    // 4. Somebody else's queue is not this reader's. A queue is per account, so the same work reads differently to two
    //    readers — which is exactly what a per-work read has to get right.
    let (status, body) = other
        .get(format!("/api/v1/me/marked-for-later/{work}"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["marked"],
        json!(false),
        "another reader has not queued it: {body}"
    );

    // 5. Removing it moves the answer back, and removing nothing is not an error.
    let (status, body) = reader
        .request(
            "DELETE",
            &format!("/api/v1/works/{work}/marked-for-later"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = reader
        .get(format!("/api/v1/me/marked-for-later/{work}"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["marked"], json!(false), "{body}");

    // 6. A work that does not exist is refused by name, as the write door already does: `marked: false` for a typo'd id
    //    would be indistinguishable from "not saved", and the reader would never learn the id was wrong.
    let (status, body) = reader
        .get("/api/v1/me/marked-for-later/11111111-2222-3333-4444-555555555555")
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // 7. A queue belongs to an account; there is no anonymous version of "mine".
    let mut anonymous = TestClient::new(build(&config, &db));
    let (status, body) = anonymous
        .get(format!("/api/v1/me/marked-for-later/{work}"))
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
}
