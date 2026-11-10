//! A DNF setting with no door and a feedback list with no viewer (M70-13 pass 46).
//!
//! `works.allow_dnf_feedback` is an author opt-in — `BOOLEAN NOT NULL DEFAULT FALSE` since migration 0073 — and before this
//! pass it was read by exactly one function, `work_allows_dnf_feedback`, which nothing called. Two consequences, both
//! invisible from the product:
//!
//! * **No author could turn it on.** There was no writer for the column anywhere in the tree: not the work patch, not a
//!   settings route, not the operator console. The setting existed in the schema and could only be changed with `psql`.
//! * **The rule it names was never applied.** A reader could publish "I stopped reading this" about any work, including one
//!   whose author had (somehow) said no. A setting that no writer sets and no reader reads is a comment.
//!
//! `list_dnf_for_work` was the other half: the store could list a work's feedback and no surface showed it, so the author
//! whose work was being dropped had no way to learn why — while `aggregate_dnf_counts` (wired to
//! `GET /works/{id}/dnf/reasons`) showed them the *numbers* and none of the sentences behind them.
//!
//! # What this pass decides, and why
//!
//! **A private DNF stays private, including from the author.** `is_public` defaults to false (M45-21: "private by
//! default"), and the reader who leaves it false has said "this is for me". An author is not an exception to that: the
//! record is the reader's own reading history, not the work's property. The author sees the published feedback and the
//! aggregate; the argument `include_private` is gone rather than defaulted, because a parameter that must always be false
//! is a parameter that will eventually be passed true by someone who reads its name and not its history.
//!
//! **The opt-in gates publication, not recording.** A reader may always record that they stopped reading — that is their
//! own history, and refusing it would make the library lie about what they read. What the author controls is whether that
//! becomes a public statement *about their work*, which is the thing `allow_dnf_feedback` names.

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

/// Publish a work with one chapter, returning its id. Both halves are required: an empty work has nothing to read and the
/// publication is refused by name.
async fn published_work(author: &mut TestClient) -> String {
    let (status, body) = author
        .post("/api/v1/works", json!({ "title": "Dropped" }))
        .await;
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
                    { "type": "paragraph", "content": [{ "type": "text", "text": "It began well enough." }] },
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
            json!({ "expected_version": version, "idempotency_key": format!("dnf-{work}") }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    work
}

#[tokio::test]
async fn a_work_that_does_not_accept_feedback_does_not_get_public_feedback() {
    let dir = scratch_dir("dnf-feedback");
    let tdb = TestDb::connect_with_dir("m7013-dnf-feedback", &dir).await;
    let db: Database = tdb.db().clone();
    let config = Config::development_defaults();

    let mut author = TestClient::new(build(&config, &db));
    register(&mut author, "dnf-author@example.test", "dnfauthor").await;
    let mut reader = TestClient::new(build(&config, &db));
    register(&mut reader, "dnf-reader@example.test", "dnfreader").await;
    let mut stranger = TestClient::new(build(&config, &db));
    register(&mut stranger, "dnf-stranger@example.test", "dnfstranger").await;

    let work = published_work(&mut author).await;

    // 1. The column's default is FALSE, so a work accepts no public feedback until its author says otherwise. Before this
    //    pass the row was written anyway and the setting had no effect on anything.
    let (status, body) = reader
        .post(
            &format!("/api/v1/works/{work}/dnf"),
            json!({ "reason": "slow_pacing", "note": "the pacing lost me", "is_public": true }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a public DNF needs the author's opt-in: {body}"
    );
    assert_eq!(
        body["error"]["field_errors"]["is_public"].is_string(),
        true,
        "the refusal names the field that asked for publication, not the work or the reason: {body}"
    );

    // 2. A private DNF is the reader's own history and needs no permission — including on a work with the opt-in off. If
    //    this were refused, the library would forget what the reader read on every work whose author never touched a
    //    setting.
    let (status, body) = reader
        .post(
            &format!("/api/v1/works/{work}/dnf"),
            json!({ "reason": "slow_pacing", "note": "private: I will come back to it", "is_public": false }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a private record is not publication: {body}"
    );
    assert_eq!(body["is_public"], json!(false), "{body}");

    // 3. The author can now turn the setting on — through a door, which did not exist. `allow_dnf_feedback` was a column
    //    with no writer.
    let (_, work_body) = author.get(format!("/api/v1/works/{work}")).await;
    let version = work_body["version"].as_i64().expect("work version");
    let (status, body) = author
        .request(
            "PATCH",
            &format!("/api/v1/works/{work}"),
            Some(json!({ "expected_version": version, "allow_dnf_feedback": true })),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the author turns feedback on: {body}"
    );

    let (status, body) = reader
        .post(
            &format!("/api/v1/works/{work}/dnf"),
            json!({ "reason": "slow_pacing", "note": "the pacing lost me", "is_public": true }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "now it may be published: {body}");

    // 3b. A second reader drops the same work privately, and their row coexists with the published one. Without this the
    //     exclusion below proves nothing: step 2's private row belongs to the same pseud as the public one in step 3, and
    //     the upsert would have replaced it — a list with no filter at all would still show one row, and the mutant that
    //     removes `is_public = TRUE` survived the first version of this test.
    let mut quiet = TestClient::new(build(&config, &db));
    register(&mut quiet, "dnf-quiet@example.test", "dnfquiet").await;
    let (status, body) = quiet
        .post(
            &format!("/api/v1/works/{work}/dnf"),
            json!({ "reason": "not_my_taste", "note": "private: I will come back to it", "is_public": false }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // 4. The author reads the feedback: the published sentence is there, the private one is not. The aggregate door shows
    //    counts and the author needs the reasons, which is the whole point of structured DNF.
    let (status, body) = author
        .get(format!("/api/v1/works/{work}/dnf/feedback"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let items = body["items"].as_array().expect("items");
    assert_eq!(items.len(), 1, "only the published record: {body}");
    assert_eq!(items[0]["note"], json!("the pacing lost me"), "{body}");
    assert_eq!(items[0]["reason"], json!("slow_pacing"), "{body}");
    assert!(
        !body.to_string().contains("private: I will come back to it"),
        "a private DNF is not shown to the author either: {body}"
    );
    // The reader's identity is in the row because the author must be able to answer it — this is the one surface where a
    // reader's pseudonym is the point, and it is opt-in publication that put it there.
    assert!(items[0]["pseud_id"].is_string(), "{body}");

    // 5. Nobody else reads the feedback. A stranger is told the work has no such surface, not that they may not use it:
    //    the refusal carries no information about the author's setting.
    let (status, body) = stranger
        .get(format!("/api/v1/works/{work}/dnf/feedback"))
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "feedback is the author's view: {body}"
    );

    // 6. The work's own feedback list answers for a work that exists even when empty, and refuses a work that does not.
    let (status, body) = author
        .get("/api/v1/works/11111111-2222-3333-4444-555555555555/dnf/feedback")
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // 7. Turning the setting back off stops new publications and keeps the ones already published: a statement made while
    //    the door was open is evidence, not a permission that can be revoked retroactively.
    let (_, work_body) = author.get(format!("/api/v1/works/{work}")).await;
    let version = work_body["version"].as_i64().expect("work version");
    let (status, body) = author
        .request(
            "PATCH",
            &format!("/api/v1/works/{work}"),
            Some(json!({ "expected_version": version, "allow_dnf_feedback": false })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = author
        .get(format!("/api/v1/works/{work}/dnf/feedback"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["items"].as_array().expect("items").len(),
        1,
        "the record survives the setting that allowed it: {body}"
    );
    let _: Option<Value> = None;
}
