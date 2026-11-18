//! M64-11 — the public route behind "Part of N reading lists".

use axum::body::Body;
use axum::http::{Request, StatusCode};
use darkhold_app::{config::Config, server, state::AppState};
use darkhold_db::user_list;
use darkhold_domain::user_list::{AddUserListItemRequest, CreateUserListRequest};
use serde_json::Value;
use test_support::TestDb;
use tower::ServiceExt;

const OWNER: &str = "01955100-0000-4000-8000-000000000001";
const PSEUD: &str = "01955100-0000-4000-8000-000000000004";
const WORK: &str = "01955100-0000-4000-8000-000000000005";

async fn exec(db: &darkhold_db::Database, sql: &str) {
    match db.backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(sql)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("fixture");
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(sql)
                .execute(db.postgres_pool().expect("postgres"))
                .await
                .expect("fixture");
        }
    }
}

#[tokio::test]
async fn the_work_door_publishes_only_public_lists_without_a_session() {
    let dir = std::env::temp_dir().join(format!("darkhold-m64-11-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let tdb = TestDb::connect_with_dir("lists-containing-route", &dir).await;
    let db = tdb.db();
    exec(db, &format!("INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version) VALUES ('{OWNER}','active','owner@test.dev','adult','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)")).await;
    exec(db, &format!("INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at, updated_at, version) VALUES ('{PSEUD}','{OWNER}','owner','owner','listed','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)")).await;
    exec(db, &format!("INSERT INTO works (id, owner_pseud_id, title, visibility, lifecycle, created_at, updated_at) VALUES ('{WORK}','{PSEUD}','One','public','published','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')")).await;

    for (name, visibility) in [("Public shelf", "public"), ("Private shelf", "private")] {
        let list = user_list::create(
            db,
            OWNER,
            &CreateUserListRequest {
                name: name.to_owned(),
                description: String::new(),
                visibility: Some(visibility.to_owned()),
                moderation: None,
                criteria: None,
            },
        )
        .await
        .expect("create list");
        user_list::add_item(
            db,
            &list.id,
            OWNER,
            &AddUserListItemRequest {
                work_id: WORK.to_owned(),
                note: String::new(),
            },
        )
        .await
        .expect("add item");
    }

    let app = server::build_router(AppState::new(Config::development_defaults(), db.clone()));
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/works/{WORK}/lists"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("body");
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(body["work_id"], WORK);
    let names: Vec<&str> = body["lists"]
        .as_array()
        .expect("lists")
        .iter()
        .map(|list| list["name"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(names, vec!["Public shelf"], "body: {body}");
}
