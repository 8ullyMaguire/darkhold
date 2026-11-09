//! A tag's confirmation, and the door that was missing (M70-13).
//!
//! §49.2 says only reader- or wrangler-confirmed tags may move a reader's ranking, and `tag_confirmation.rs` implements exactly
//! that: `counts_toward_gravity` is true for `Reader` and `Wrangler`, false for `Unconfirmed` and `Inaccurate`, and
//! `gravity_contributing_tags` is what discovery, tasting and the concierge read when they build a gravity vector.
//!
//! `tag_confirmation::confirm` had no caller. Measured on the deployed database: **30,982 of 30,982 `work_tags` rows are
//! `unconfirmed`** — so the filter returned nothing for every work, and §49.2's rule reduced to "no tag moves any ranking". The
//! gravity layer was fed zero tags, silently, on every surface that reads it. Two other passes had already found the same layer
//! starved of input (the taste signal, and the tags themselves); this is the third and the only one where the product's own rule
//! names the missing act.
//!
//! Four decisions, each recorded beside the code:
//!
//! * **A reader may confirm or flag; only a curator may claim `wrangler`.** §49.2 gives the reader two acts and the wrangler one,
//!   and a state is a claim about authority — a reader who could set `wrangler` would launder a scoped opinion into a curation.
//! * **`unconfirmed` is refused.** It is the absence of a decision (the state every imported row is in), not something a person
//!   can assert; a request that sends it is asking to erase a judgement, which is a different feature with a different audit.
//! * **A tag the work does not carry answers `404`.** Same rule as the vote door beside it: there is no claim to judge, and `422`
//!   would blame a well-formed request.
//! * **Flagging stores who flagged.** The column exists for the queue that reviews it; a flag with no flagger is a signal nobody
//!   can act on.

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

/// The tags the gravity layer would use, as the product reads them.
async fn contributing(db: &Database, work: &str) -> Vec<String> {
    darkhold_db::tag_confirmation::gravity_contributing_tags(
        db,
        work,
        darkhold_db::tag_confirmation::DEFAULT_CONTRIBUTION_CAP,
    )
    .await
    .expect("gravity tags")
}

#[tokio::test]
async fn a_reader_can_confirm_a_tag_and_it_starts_moving_gravity() {
    let dir = scratch_dir("tag-confirmation");
    let tdb = TestDb::connect_with_dir("m7013-tag-confirmation", &dir).await;
    let db: Database = tdb.db().clone();
    let config = Config::development_defaults();

    let mut author = TestClient::new(build(&config, &db));
    register(&mut author, "tc-author@example.test", "tcauthor").await;
    let work = a_work(&mut author, "Confirmed").await;
    let node = a_node(&mut author, "angst").await;
    let (status, body) = author
        .post(
            &format!("/api/v1/works/{work}/tags"),
            json!({ "node_id": node }),
        )
        .await;
    assert!(status.is_success(), "the work carries the tag: {body}");

    // 1. Author-applied and nothing more: it is displayed, searchable, and counts toward nothing. That is the state every row on
    //    the deployed instance is in, and it is why the gravity layer has been reading an empty set.
    assert!(
        contributing(&db, &work).await.is_empty(),
        "an unconfirmed tag must not move gravity"
    );

    let mut reader = TestClient::new(build(&config, &db));
    register(&mut reader, "tc-reader@example.test", "tcreader").await;

    let door = format!("/api/v1/works/{work}/tags/{node}/confirmation");

    // 2. A reader says the tag is right. This is the act the whole module was waiting for.
    let (status, body) = reader
        .request("PUT", &door, Some(json!({ "state": "reader" })))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["state"], json!("reader"), "{body}");
    // The filter answers with tag NAMES, not node ids: it feeds a gravity vector of words a reader would recognise.
    assert_eq!(
        contributing(&db, &work).await,
        vec!["angst".to_owned()],
        "a reader-confirmed tag is what the gravity layer reads"
    );

    // 3. A reader flags it instead, and the flag records who — the queue that reviews it needs a name, and the tag stops counting
    //    immediately rather than when somebody gets to the queue (§49.2's "should not keep paying while a queue works").
    let (status, body) = reader
        .request("PUT", &door, Some(json!({ "state": "inaccurate" })))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        contributing(&db, &work).await.is_empty(),
        "a flagged tag must stop counting at once"
    );
    let flagged: Option<String> = match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar(
            "SELECT flagged_by FROM work_tags WHERE work_id = ? AND node_id = ?",
        )
        .bind(&work)
        .bind(&node)
        .fetch_one(db.sqlite_pool().expect("sqlite"))
        .await
        .expect("flagged_by"),
        darkhold_db::Backend::Postgres => sqlx::query_scalar(
            "SELECT flagged_by FROM work_tags WHERE work_id::text = $1 AND node_id = $2",
        )
        .bind(&work)
        .bind(&node)
        .fetch_one(db.postgres_pool().expect("postgres"))
        .await
        .expect("flagged_by"),
    };
    assert!(
        flagged.is_some_and(|who| !who.is_empty()),
        "a flag with no flagger is a signal nobody can act on"
    );

    // 4. `wrangler` is authority, and a reader may not claim it. The state is left as it was: a refused claim must not half-apply.
    let (status, body) = reader
        .request("PUT", &door, Some(json!({ "state": "wrangler" })))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(
        contributing(&db, &work).await.is_empty(),
        "a refused wrangler claim must not make the tag count: {body}"
    );

    // 5. `unconfirmed` is the absence of a judgement, not a judgement; sending it is refused by name rather than erasing what a
    //    person said.
    let (status, body) = reader
        .request("PUT", &door, Some(json!({ "state": "unconfirmed" })))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body.to_string().contains("state"), "{body}");

    // 6. A tag the work does not carry has no claim to judge, and the answer is 404 — the vote door beside it settled this.
    let other = a_node(&mut author, "fluff").await;
    let (status, body) = reader
        .request(
            "PUT",
            &format!("/api/v1/works/{work}/tags/{other}/confirmation"),
            Some(json!({ "state": "reader" })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}
