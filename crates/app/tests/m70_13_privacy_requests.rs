//! A privacy request a person can file and nobody could finish (M70-13 pass 64).
//!
//! `POST /me/privacy-requests` has existed since 0021 and `GET` beside it lets a person see their own. But
//! `admin::complete_privacy_request` had **no caller**, and there was no operator door that listed requests at all — so a
//! `delete` request sat in `pending` forever, the person asking could see it and watch it never move, and the only way to
//! finish one was a `psql` statement nobody had written down.
//!
//! That is the queue-with-no-action shape again, and here the queue is a legal obligation rather than a nicety: an erasure
//! request that never leaves `pending` is a promise the instance made and cannot keep.
//!
//! Four decisions:
//!
//! * **Completion is operator-only and takes the request's account with it.** The answer echoes whose request it was, because
//!   the operator's question is "whose obligation did I just close", and a completion that cannot say is unauditable.
//! * **Only `pending` completes.** A second completion would rewrite `completed_at` and `result_ref` — the two columns that make
//!   the record evidence. It answers `422` naming the state it is in, not a silent second success.
//! * **An unknown request is `404`.** A request id is not a capability: it tells the caller nothing about another person's
//!   request, and there is no reason an operator needs an existence oracle either.
//! * **The queue lists every account and filters by state.** A queue that only shows one account is a queue no operator can
//!   drain.

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

#[tokio::test]
async fn a_person_files_a_delete_and_an_operator_finishes_it() {
    let dir = scratch_dir("privacy-requests");
    let tdb = TestDb::connect_with_dir("m7013-privacy-requests", &dir).await;
    let db: Database = tdb.db().clone();
    let config = Config::development_defaults();

    // 1. A person asks for their data to be deleted. They can already do this, and they can already watch it stay pending.
    let mut person = TestClient::new(build(&config, &db));
    let account = register(&mut person, "pr@example.test", "prperson").await;
    let (status, body) = person
        .post("/api/v1/me/privacy-requests", json!({ "kind": "delete" }))
        .await;
    assert!(status.is_success(), "the request is filed: {body}");
    let request_id = body["id"].as_str().expect("request id").to_owned();

    // 2. The operator can see the queue. Until this pass there was no door that listed requests at all, so the only way to find
    //    one was to read the table.
    let mut operator = TestClient::new(build(&config, &db));
    let op_account = register(&mut operator, "pr-op@example.test", "prop").await;
    let mut admin = Config::development_defaults();
    admin.administration.operator_account_id = Some(op_account.parse().expect("account id"));
    let mut operator = TestClient::new(build(&admin, &db));
    test_support::sign_in_as(&mut operator, &tdb, "pr-op@example.test", "prop").await;

    let (status, body) = operator
        .get("/api/v1/admin/privacy-requests?state=pending")
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let queue = body["requests"].as_array().expect("requests");
    let mine = queue
        .iter()
        .find(|row| row["id"] == json!(request_id))
        .unwrap_or_else(|| panic!("the request is in the operator's queue: {body}"));
    assert_eq!(mine["state"], json!("pending"), "{body}");
    assert_eq!(mine["kind"], json!("delete"), "{body}");
    assert_eq!(
        mine["account"],
        json!(account),
        "the queue says whose obligation it is: {body}"
    );

    // 3. The operator finishes it, with the reference that makes the completion evidence rather than a checkbox.
    let (status, body) = operator
        .post(
            &format!("/api/v1/admin/privacy-requests/{request_id}/complete"),
            json!({ "result_ref": "erasure:2026-11-09/case-41" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["state"], json!("done"), "{body}");
    assert_eq!(
        body["result_ref"],
        json!("erasure:2026-11-09/case-41"),
        "{body}"
    );
    assert!(
        body["completed_at"].as_str().is_some_and(|s| !s.is_empty()),
        "a completion is dated: {body}"
    );

    // 4. And the person who asked can see it moved. This is the half that was missing: the same row, answered by the door they
    //    already had.
    let (status, body) = person.get("/api/v1/me/privacy-requests").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let theirs = body["requests"]
        .as_array()
        .expect("requests")
        .iter()
        .find(|row| row["id"] == json!(request_id))
        .cloned()
        .expect("their request");
    assert_eq!(theirs["state"], json!("done"), "{body}");
    assert_eq!(
        theirs["result_ref"],
        json!("erasure:2026-11-09/case-41"),
        "the person gets the reference, not just a status: {body}"
    );

    // 5. A second completion does not rewrite the record. `completed_at` and `result_ref` are the evidence; a second write would
    //    make them say whichever operator came last.
    let (status, body) = operator
        .post(
            &format!("/api/v1/admin/privacy-requests/{request_id}/complete"),
            json!({ "result_ref": "erasure:second" }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body.to_string().contains("done"),
        "the refusal names the state it is in: {body}"
    );
    let (_, body) = person.get("/api/v1/me/privacy-requests").await;
    let theirs = body["requests"]
        .as_array()
        .expect("requests")
        .iter()
        .find(|row| row["id"] == json!(request_id))
        .cloned()
        .expect("their request");
    assert_eq!(
        theirs["result_ref"],
        json!("erasure:2026-11-09/case-41"),
        "the first completion stands: {body}"
    );

    // 6. An unknown request is `404`: an id is not a capability, and the queue is not an existence oracle.
    let (status, _) = operator
        .post(
            "/api/v1/admin/privacy-requests/99999999-9999-9999-9999-999999999999/complete",
            json!({ "result_ref": "x" }),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // 7. `done` requests leave the pending queue and stay visible when asked for by state — a drained queue and a deleted
    //    record have to be distinguishable to the operator doing the draining.
    let (status, body) = operator
        .get("/api/v1/admin/privacy-requests?state=pending")
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        !body["requests"]
            .as_array()
            .expect("requests")
            .iter()
            .any(|row| row["id"] == json!(request_id)),
        "a finished request is not still pending: {body}"
    );
    let (status, body) = operator
        .get("/api/v1/admin/privacy-requests?state=done")
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["requests"]
            .as_array()
            .expect("requests")
            .iter()
            .any(|row| row["id"] == json!(request_id)),
        "and it is still there when asked for: {body}"
    );

    // 8. A person who is not the operator cannot read the queue or complete anything. `404`, because the answer to "does this
    //    door exist for me" is the same as for every other door they may not open.
    let (status, _) = person.get("/api/v1/admin/privacy-requests").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = person
        .post(
            &format!("/api/v1/admin/privacy-requests/{request_id}/complete"),
            json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // 9. And with no session at all, the queue is not a public list of who asked to be deleted.
    let mut anon = TestClient::new(build(&config, &db));
    let (status, _) = anon.get("/api/v1/admin/privacy-requests").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    tdb.cleanup().await;
}
