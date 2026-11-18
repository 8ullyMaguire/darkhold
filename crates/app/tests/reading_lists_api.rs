//! Row 413 — the reading-list API proof: a list with a collaborator, read by
//! the collaborator, refused to a stranger, and deleted with its children.
//!
//! The doors are HTTP doors; the permissions are not re-derived in the handler.
//! The same test also checks that a private list and a missing list answer the
//! same 404, and that the service's `chosen_mods` rule reaches the wire.

use axum::http::StatusCode;
use darkhold_app::{config::Config, server, state::AppState};
use darkhold_db::{identity, Backend};
use darkhold_domain::policy::AgeState;
use serde_json::{json, Value};
use sqlx::Row;

fn router_for(root: &std::path::Path, db: &darkhold_db::Database) -> axum::Router {
    let mut config = Config::development_defaults();
    config.storage.root = root.to_path_buf();
    server::build_router(AppState::new(config, db.clone()))
}

async fn member(router: &axum::Router, tag: &str) -> (test_support::TestClient, String) {
    let mut client = test_support::TestClient::new(router.clone());
    let account = test_support::register(&mut client, &format!("row413-{tag}@test.dev"), tag).await;
    (client, account)
}

async fn seed_work(db: &darkhold_db::Database, tag: &str) -> String {
    let account = identity::create_account(
        db,
        &format!("row413-author-{tag}@test.dev"),
        AgeState::DeclaredAdult,
        identity::AccountStatus::Active,
    )
    .await
    .expect("author account");
    let pseud = identity::create_pseud(db, account, "author", "Author")
        .await
        .expect("author pseud");
    let work = darkhold_db::content::create_work(db, pseud, "A work", Some("public"))
        .await
        .expect("work");
    work.id.to_string()
}

async fn child_counts(db: &darkhold_db::Database, list_id: &str) -> (i64, i64) {
    let sql = db.sql(
        "SELECT (SELECT count(*) FROM user_list_members WHERE list_id = ?), \
                (SELECT count(*) FROM user_list_collaborators WHERE list_id = ?)",
        "SELECT (SELECT count(*) FROM user_list_members WHERE list_id = $1::uuid), \
                (SELECT count(*) FROM user_list_collaborators WHERE list_id = $1::uuid)",
    );
    match db.backend() {
        Backend::Sqlite => {
            let row = sqlx::query(&sql)
                .bind(list_id)
                .bind(list_id)
                .fetch_one(db.sqlite_pool().expect("sqlite handle"))
                .await
                .expect("count children");
            (row.get::<i64, _>(0), row.get::<i64, _>(1))
        }
        Backend::Postgres => {
            let row = sqlx::query(&sql)
                .bind(list_id)
                .fetch_one(db.postgres_pool().expect("postgres handle"))
                .await
                .expect("count children");
            (row.get::<i64, _>(0), row.get::<i64, _>(1))
        }
    }
}

#[tokio::test]
async fn reading_list_api_collaborator_read_and_cascade() {
    let root = std::env::temp_dir().join(format!("row413-api-{}", std::process::id()));
    std::fs::create_dir_all(&root).expect("scratch root");
    let tdb = test_support::TestDb::connect_with_dir("row413-api", &root).await;
    let db = tdb.db();
    let router = router_for(&root, db);
    let work = seed_work(db, "api").await;

    let (mut owner, owner_id) = member(&router, "owner").await;
    let (mut curator, curator_id) = member(&router, "curator").await;
    let (mut stranger, _stranger_id) = member(&router, "stranger").await;

    // create
    let (status, body) = owner
        .post(
            "/api/v1/reading-lists",
            json!({
                "name": "  Autumn TBR  ",
                "description": "things to read",
                "visibility": "private",
                "moderation": "chosen_mods"
            }),
        )
        .await;
    assert!(status.is_success(), "create: {status} {body}");
    let id = body["list"]["id"]
        .as_str()
        .expect("list id in create response")
        .to_owned();
    assert_eq!(body["list"]["name"], "Autumn TBR", "name is trimmed");

    // add item, then assert it is present before testing who can see it
    let (status, body) = owner
        .post(
            &format!("/api/v1/reading-lists/{id}/items"),
            json!({ "work_id": work, "note": "first" }),
        )
        .await;
    assert!(status.is_success(), "add item: {status} {body}");
    assert_eq!(body["item"]["work_id"], json!(work));

    // add collaborator
    let (status, body) = owner
        .post(
            &format!("/api/v1/reading-lists/{id}/collaborators"),
            json!({ "user_id": curator_id, "role": "moderator" }),
        )
        .await;
    assert!(status.is_success(), "add collaborator: {status} {body}");
    assert_eq!(body["collaborator"]["role"], "moderator");

    // collaborator reads list, works and the collaborator set
    let (status, body) = curator.get(&format!("/api/v1/reading-lists/{id}")).await;
    assert_eq!(status, StatusCode::OK, "collaborator read: {body}");
    assert_eq!(body["list"]["item_count"], json!(1));
    let (status, body) = curator
        .get(&format!("/api/v1/reading-lists/{id}/items"))
        .await;
    assert_eq!(status, StatusCode::OK, "collaborator items: {body}");
    assert_eq!(body["items"].as_array().expect("items array").len(), 1);
    let (status, body) = curator
        .get(&format!("/api/v1/reading-lists/{id}/collaborators"))
        .await;
    assert_eq!(status, StatusCode::OK, "collaborator roster: {body}");
    assert_eq!(body["collaborators"].as_array().expect("roster").len(), 1);

    // non-collaborator cannot read. A missing id is the paired control: both
    // answer the same 404, so a private list's existence stays undisclosed.
    let (status, body) = stranger.get(&format!("/api/v1/reading-lists/{id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "stranger read: {body}");
    let code = body["error"]["code"].as_str().expect("error code");
    assert!(code.contains("NOT_FOUND"), "stranger code: {body}");
    let (status, body) = stranger
        .get(&format!("/api/v1/reading-lists/{id}/items"))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "stranger items: {body}");
    let missing = "01955199-0000-4000-8000-000000000099";
    let (status, absent) = stranger
        .get(&format!("/api/v1/reading-lists/{missing}"))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "missing read: {absent}");
    assert_eq!(body["error"]["code"], absent["error"]["code"]);

    // mutation gate reaches the wire from the service: neither a stranger nor
    // the owner (under single_user, on a second list) may bypass it.
    let (status, body) = stranger
        .post(
            &format!("/api/v1/reading-lists/{id}/collaborators"),
            json!({ "user_id": owner_id, "role": "curator" }),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "stranger grant: {body}");

    // update: the owner makes the list public; the stranger can now read it.
    let (status, body) = owner
        .request_with(
            "PATCH",
            &format!("/api/v1/reading-lists/{id}"),
            Some(json!({ "visibility": "public" })),
            None,
        )
        .await;
    assert!(status.is_success(), "update: {status} {body}");
    let (status, body) = stranger.get(&format!("/api/v1/reading-lists/{id}")).await;
    assert_eq!(status, StatusCode::OK, "public read: {body}");
    assert_eq!(body["list"]["visibility"], "public");

    // delete: children existed before, and all are gone afterwards.
    assert_eq!(child_counts(db, &id).await, (1, 1));
    let (status, body) = owner
        .request_with("DELETE", &format!("/api/v1/reading-lists/{id}"), None, None)
        .await;
    assert!(status.is_success(), "delete: {status} {body}");
    assert_eq!(child_counts(db, &id).await, (0, 0));
    let (status, _body) = owner.get(&format!("/api/v1/reading-lists/{id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "deleted list is gone");
}

#[tokio::test]
async fn reading_list_api_owns_the_read_shape_without_a_second_table() {
    let root = std::env::temp_dir().join(format!("row413-api-shape-{}", std::process::id()));
    std::fs::create_dir_all(&root).expect("scratch root");
    let tdb = test_support::TestDb::connect_with_dir("row413-shape", &root).await;
    let db = tdb.db();
    let router = router_for(&root, db);
    let (mut owner, _owner_id) = member(&router, "shape").await;

    let (status, body) = owner
        .post(
            "/api/v1/reading-lists",
            json!({ "name": "Quorum", "moderation": "public_quorum" }),
        )
        .await;
    assert!(status.is_success(), "create: {status} {body}");
    let id = body["list"]["id"].as_str().expect("list id").to_owned();
    let keys: Vec<&str> = body["list"]
        .as_object()
        .expect("list object")
        .keys()
        .map(String::as_str)
        .collect();
    assert!(
        keys.contains(&"visibility") && keys.contains(&"moderation") && keys.contains(&"owner_id"),
        "list shape: {body}"
    );
    let (status, body) = owner.get("/api/v1/me/reading-lists").await;
    assert_eq!(status, StatusCode::OK, "mine: {body}");
    assert_eq!(body["lists"].as_array().expect("lists").len(), 1);

    // public_quorum is storable but the service refuses even the owner.
    let (status, body): (StatusCode, Value) = owner
        .request_with(
            "PATCH",
            &format!("/api/v1/reading-lists/{id}"),
            Some(json!({ "name": "Nope" })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "quorum update: {body}");
}
