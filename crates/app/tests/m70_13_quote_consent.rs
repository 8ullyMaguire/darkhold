//! §49.4 clause 2 — consent to be quoted, which had no door for the person whose words are quoted (M70-13 pass 56).
//!
//! The module comment records the clause's real subject: *"Only notes the author has allowed to be quoted are eligible"* is a
//! slip, because the note is the READER's words about the author's work, so the consent is the reader's to give. `review.allow_quote`
//! therefore defaults to `false`, and `rec_blurbs::top_rec_blurb` filters on it while building the second summary on the work
//! page.
//!
//! `rec_blurbs::set_quote_consent` and `rec_blurbs::quote_consent` existed and had no caller. So nobody could ever grant the
//! consent the filter requires: the only reachable state of clause 2 was "quoted nobody", and the feature was invisible for
//! every work on the instance — a privacy default doing the job of a missing button.
//!
//! Two doors, both pseud-scoped, both keyed on the work rather than on a review id the client would have to guess:
//! `GET /works/{id}/reviews/quote-consent` and `PUT` of the same. A caller can therefore only ever change the consent on their
//! OWN note, and a stranger gets `404` — not `403`, which would confirm that somebody else's note exists and is quotable.
//!
//! The four assertions that matter, in order: consent is off by default (nothing appears); granting it makes the note appear
//! as the work page's second summary; withdrawing it removes it again; and consent does not publish a private note, because
//! being quotable and being public are two decisions the reader makes separately.

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

/// A published work by `author`, with one chapter.
async fn published_work(author: &mut TestClient, title: &str) -> String {
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
    let version = body["version"].as_i64().expect("version");
    let (status, body) = author
        .request(
            "PATCH",
            &format!("/api/v1/chapters/{unit}"),
            Some(json!({
                "expected_version": version,
                "document": { "type": "doc", "content": [
                    { "type": "paragraph", "content": [{ "type": "text", "text": "Words that earn a note." }] },
                ]},
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, body) = author.get(format!("/api/v1/works/{work}")).await;
    let version = body["version"].as_i64().expect("work version");
    let (status, body) = author
        .post(
            &format!("/api/v1/works/{work}/publish"),
            json!({ "expected_version": version, "idempotency_key": format!("qc-{work}") }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    work
}

/// A public review by `reader`.
async fn write_review(reader: &mut TestClient, work: &str, body: &str, is_public: bool) {
    let (status, response) = reader
        .request(
            "PUT",
            &format!("/api/v1/works/{work}/reviews"),
            Some(json!({ "body": body, "is_public": is_public, "contains_spoilers": false })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{response}");
}

/// The second summary on the PUBLIC work page, or `null`.
///
/// Read anonymously, and that is the surface that matters: a second summary is for readers deciding whether to start, and the
/// author's own view of their work is a different document (`rec_blurb` is published on the public view). Reading it as the
/// author would test a field that is not there and pass for the wrong reason at every step.
async fn blurb(db: &Database, config: &Config, work: &str) -> Value {
    let mut anonymous = TestClient::new(build(config, db));
    let (status, body) = anonymous.get(format!("/api/v1/works/{work}")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["rec_blurb"].clone()
}

#[tokio::test]
async fn consent_is_off_by_default_and_withdrawable() {
    let dir = scratch_dir("quote-consent");
    let tdb = TestDb::connect_with_dir("m7013-quote-consent", &dir).await;
    let db: Database = tdb.db().clone();
    let config = Config::development_defaults();

    let mut author = TestClient::new(build(&config, &db));
    register(&mut author, "qc-author@example.test", "qcauthor").await;
    let work = published_work(&mut author, "Quotable").await;

    let mut reader = TestClient::new(build(&config, &db));
    register(&mut reader, "qc-reader@example.test", "qcreader").await;
    write_review(
        &mut reader,
        &work,
        "The seal sequence in chapter four is the best thing in this fandom.",
        true,
    )
    .await;

    // 1. Default: no consent, and therefore no second summary. The feature being invisible is the correct state — the privacy
    //    default is doing its job — and the point of the doors below is that it is a default rather than a dead end.
    assert_eq!(
        blurb(&db, &config, &work).await,
        Value::Null,
        "a note nobody has consented to quote must not be shown beside the summary"
    );

    // 2. The reader can read their own state without writing anything.
    let (status, body) = reader
        .get(format!("/api/v1/works/{work}/reviews/quote-consent"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["allow_quote"], json!(false), "{body}");
    assert_eq!(body["work_id"], json!(work), "{body}");

    // 3. Grant it, and the note becomes the work's second summary — attributed, and the wording is the reader's own.
    let (status, body) = reader
        .request(
            "PUT",
            &format!("/api/v1/works/{work}/reviews/quote-consent"),
            Some(json!({ "allow_quote": true })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["allow_quote"], json!(true), "{body}");

    let shown = blurb(&db, &config, &work).await;
    assert_ne!(shown, Value::Null, "consent makes the note eligible");
    assert_eq!(shown["pseud_handle"], json!("qcreader"), "{shown}");
    assert!(
        shown["excerpt"]
            .as_str()
            .is_some_and(|text| text.contains("seal sequence")),
        "the excerpt is the reader's own sentence: {shown}"
    );
    assert_eq!(
        shown["review_id"], body["review_id"],
        "the second summary names the note it quotes: {shown}"
    );

    // 4. Withdraw it, and it is gone — the withdrawal is what a reader most needs to work, and it is the direction a caching
    //    bug would break silently.
    let (status, body) = reader
        .request(
            "PUT",
            &format!("/api/v1/works/{work}/reviews/quote-consent"),
            Some(json!({ "allow_quote": false })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        blurb(&db, &config, &work).await,
        Value::Null,
        "withdrawing consent removes the second summary"
    );
    let (_, listed) = author.get(format!("/api/v1/works/{work}/reviews")).await;
    assert_eq!(
        listed["items"].as_array().map(Vec::len),
        Some(1),
        "the note itself is untouched: withdrawing consent is not deleting the review: {listed}"
    );

    // 5. Consent is not publication. A private note that has consented to be quoted still does not appear: `is_public = 1` is
    //    the other filter, and the two decisions are separate.
    write_review(&mut reader, &work, "Private thoughts, quoted never.", false).await;
    let (status, body) = reader
        .request(
            "PUT",
            &format!("/api/v1/works/{work}/reviews/quote-consent"),
            Some(json!({ "allow_quote": true })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        blurb(&db, &config, &work).await,
        Value::Null,
        "a private note is not quotable however willing its writer is: consent is not publication"
    );

    // 6. Somebody else's note is not addressable. A second reader has written no note here, so both doors answer `404` —
    //    the same answer as a work they cannot see, because a `403` would confirm that a quotable note exists.
    let mut stranger = TestClient::new(build(&config, &db));
    register(&mut stranger, "qc-stranger@example.test", "qcstranger").await;
    let (status, body) = stranger
        .request(
            "PUT",
            &format!("/api/v1/works/{work}/reviews/quote-consent"),
            Some(json!({ "allow_quote": true })),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a stranger cannot consent on somebody else's note: {body}"
    );
    let (status, body) = stranger
        .get(format!("/api/v1/works/{work}/reviews/quote-consent"))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // And the note they tried to speak for is unchanged.
    let (status, body) = reader
        .get(format!("/api/v1/works/{work}/reviews/quote-consent"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["allow_quote"], json!(true), "{body}");
}
