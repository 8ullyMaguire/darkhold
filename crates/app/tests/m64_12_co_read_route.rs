//! M64-12 — the public co-read door enforces the same privacy floor as the store.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use darkhold_app::{config::Config, server, state::AppState};
use serde_json::Value;
use test_support::TestDb;
use tower::ServiceExt;

const A: &str = "01955700-0000-4000-8000-000000000001";
const B: &str = "01955700-0000-4000-8000-000000000002";
const ACCOUNT: &str = "01955700-0000-4000-8000-000000000011";
const PSEUD: &str = "01955700-0000-4000-8000-000000000012";

#[tokio::test]
async fn also_enjoyed_is_public_and_reports_only_aggregates() {
    let dir = std::env::temp_dir().join(format!("darkhold-m64-12-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let tdb = TestDb::connect_with_dir("also-enjoyed-route", &dir).await;
    let db = tdb.db();
    let exec = |sql: String| async move {
        match db.backend() {
            darkhold_db::Backend::Sqlite => {
                sqlx::query(&sql)
                    .execute(db.sqlite_pool().expect("sqlite"))
                    .await
                    .expect("fixture");
            }
            darkhold_db::Backend::Postgres => {
                sqlx::query(&sql)
                    .execute(db.postgres_pool().expect("postgres"))
                    .await
                    .expect("fixture");
            }
        }
    };
    exec(format!("INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version) VALUES ('{ACCOUNT}','active','reader@test.dev','adult','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)")).await;
    exec(format!("INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at, updated_at, version) VALUES ('{PSEUD}','{ACCOUNT}','reader','Reader','listed','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)")).await;
    for (id, title) in [(A, "A"), (B, "B")] {
        exec(format!("INSERT INTO works (id, owner_pseud_id, title, visibility, lifecycle, created_at, updated_at) VALUES ('{id}','{PSEUD}','{title}','public','published','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')")).await;
    }
    exec(format!("INSERT INTO reading_sessions (id, account_id, work_id, seconds, started_at, ended_at) VALUES ('01955700-0000-4000-8000-000000000101','{ACCOUNT}','{A}',60,'2026-11-10T10:00:00Z','2026-11-10T10:01:00Z')")).await;
    exec(format!("INSERT INTO reading_sessions (id, account_id, work_id, seconds, started_at, ended_at) VALUES ('01955700-0000-4000-8000-000000000102','{ACCOUNT}','{B}',90,'2026-11-10T10:05:00Z','2026-11-10T10:06:30Z')")).await;

    let app = server::build_router(AppState::new(Config::development_defaults(), db.clone()));
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/works/{A}/also-enjoyed?min_readers=1"))
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
    assert_eq!(body["work_id"], A);
    assert_eq!(body["works"][0]["work_id"], B);
    assert_eq!(body["works"][0]["readers"], 1);
    assert!(body.to_string().contains("readers"));
}
