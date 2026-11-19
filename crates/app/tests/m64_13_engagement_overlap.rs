//! M64-13 — the overlap door never publishes a sub-k count.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use darkhold_app::{config::Config, server, state::AppState};
use serde_json::Value;
use test_support::TestDb;
use tower::ServiceExt;

const A: &str = "01955700-0000-4000-8000-000000000001";
const B: &str = "01955700-0000-4000-8000-000000000002";
const C: &str = "01955700-0000-4000-8000-000000000003";

#[tokio::test]
async fn engagement_overlap_is_public_and_suppressed_below_the_floor() {
    let dir = std::env::temp_dir().join(format!("darkhold-m64-13-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let tdb = TestDb::connect_with_dir("overlap-route", &dir).await;
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
    let owner = "01955700-0000-4000-8000-0000000000ff";
    let owner_pseud = "01955700-0000-4000-8000-0000000000fe";
    exec(format!("INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version) VALUES ('{owner}','active','owner@test.dev','adult','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)")).await;
    exec(format!("INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at, updated_at, version) VALUES ('{owner_pseud}','{owner}','owner','Owner','listed','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)")).await;
    for (id, title) in [(A, "A"), (B, "B"), (C, "C")] {
        exec(format!("INSERT INTO works (id, owner_pseud_id, title, visibility, lifecycle, created_at, updated_at) VALUES ('{id}','{owner_pseud}','{title}','public','published','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')")).await;
    }
    for index in 0..10 {
        let account = format!("01955700-0000-4000-8000-0000000001{:02}", index);
        let pseud = format!("01955700-0000-4000-8000-0000000002{:02}", index);
        exec(format!("INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version) VALUES ('{account}','active','r{index}@test.dev','adult','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)")).await;
        exec(format!("INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at, updated_at, version) VALUES ('{pseud}','{account}','r{index}','R{index}','listed','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)")).await;
        let a_session = format!("01955700-0000-4000-8000-0000000003{:02}", index);
        exec(format!("INSERT INTO reading_sessions (id, account_id, work_id, seconds, started_at, ended_at) VALUES ('{a_session}','{account}','{A}',60,'2026-11-10T10:00:00Z','2026-11-10T10:01:00Z')")).await;
        let b_session = format!("01955700-0000-4000-8000-0000000004{:02}", index);
        exec(format!("INSERT INTO reading_sessions (id, account_id, work_id, seconds, started_at, ended_at) VALUES ('{b_session}','{account}','{B}',90,'2026-11-10T10:05:00Z','2026-11-10T10:06:30Z')")).await;
        if index < 9 {
            let c_session = format!("01955700-0000-4000-8000-0000000005{:02}", index);
            exec(format!("INSERT INTO reading_sessions (id, account_id, work_id, seconds, started_at, ended_at) VALUES ('{c_session}','{account}','{C}',90,'2026-11-10T10:05:00Z','2026-11-10T10:06:30Z')")).await;
        }
    }

    let app = server::build_router(AppState::new(Config::development_defaults(), db.clone()));
    for (with, expected) in [
        (B, Value::from(10)),
        (C, serde_json::json!({"fewer_than": 10})),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/works/{A}/engagement-overlap?with={with}"))
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
        assert_eq!(body["with"], with);
        assert_eq!(body["overlap"], expected, "body: {body}");
        assert!(
            !body.to_string().contains("account"),
            "reader ids must not leak: {body}"
        );
    }
}
