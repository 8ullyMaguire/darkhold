//! M45-76 Step 5.2: an owner's completion edit carries provenance.
//!
//! The columns on `works` are authoritative; the history row is best-effort.
//! This test asserts both through the real PATCH route so a store-only change
//! that no caller reaches cannot pass.

use axum::http::StatusCode;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use serde_json::json;
use test_support::{scratch_dir, sign_in_as, TestClient, TestDb};

const WORK: &str = "01957600-0000-4000-8000-00000000a003";

async fn exec(db: &darkhold_db::Database, sql: &str) {
    sqlx::query(sql)
        .execute(db.sqlite_pool().expect("sqlite handle"))
        .await
        .expect("fixture");
}

#[tokio::test]
async fn an_owner_completion_edit_records_owner_provenance() {
    let dir = scratch_dir("owner_provenance");
    let tdb = TestDb::connect_with_dir("owner_provenance", &dir).await;
    let mut config = Config::development_defaults();
    config.storage.root = dir;
    let mut owner = TestClient::new(server::build_router(AppState::new(
        config,
        tdb.db().clone(),
    )));
    let account = sign_in_as(
        &mut owner,
        &tdb,
        "owner-provenance@example.com",
        "ownerprov",
    )
    .await;
    let pseud: String = sqlx::query_scalar("SELECT id FROM pseuds WHERE account_id = ?")
        .bind(&account)
        .fetch_one(tdb.db().sqlite_pool().expect("sqlite handle"))
        .await
        .expect("owner pseud");
    exec(
        tdb.db(),
        &format!(
            "INSERT INTO works (id, owner_pseud_id, title, visibility, lifecycle, completion, created_at, updated_at, version) VALUES ('{WORK}','{pseud}','Owned','public','published','in_progress','2001-01-01T00:00:00Z','2001-01-01T00:00:00Z',1)"
        ),
    )
    .await;
    exec(
        tdb.db(),
        &format!(
            "INSERT INTO work_contributors (work_id, pseud_id, role, public_attribution, created_at) VALUES ('{WORK}','{pseud}','owner',1,'2001-01-01T00:00:00Z')"
        ),
    )
    .await;

    let (status, body) = owner
        .request(
            "PATCH",
            &format!("/api/v1/works/{WORK}"),
            Some(json!({ "expected_version": 1, "completion": "complete" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "owner PATCH: {body}");

    let (source, marked): (String, Option<String>) = sqlx::query_as(
        "SELECT completion_source, completion_marked_at FROM works WHERE id = '01957600-0000-4000-8000-00000000a003'",
    )
    .fetch_one(tdb.db().sqlite_pool().expect("sqlite handle"))
    .await
    .expect("read provenance");
    assert_eq!(source, "owner");
    assert!(
        marked.is_some(),
        "the mark time must be written with the state"
    );

    let marks: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM work_completion_marks WHERE work_id = '01957600-0000-4000-8000-00000000a003' AND source = 'owner'",
    )
    .fetch_one(tdb.db().sqlite_pool().expect("sqlite handle"))
    .await
    .expect("read marks");
    assert_eq!(marks, 1, "one authoritative owner mark");

    tdb.cleanup().await;
}
