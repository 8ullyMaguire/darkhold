//! The batch-window and resonance doors (M70-13, giving §52.2/§52.3's store a surface).
//!
//! The window is not bookkeeping: §52.2's promise is that a payout cannot be correlated with an individual reader
//! *because* it lands after a window closes and covers many readers. So these tests assert the boundaries — one open
//! window at a time, a label only against a closed window, a label readable only by its owner or an operator.

use axum::http::StatusCode;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::{Backend, Database};
use test_support::{active_pseud_id, id, register, scratch_dir, sign_in_as, TestClient, TestDb};

async fn insert_work(tdb: &TestDb, work_id: &str, pseud: &str) {
    let now = "2026-10-01T12:00:00Z";
    match tdb.db().backend() {
        Backend::Sqlite => {
            sqlx::query("INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at) VALUES (?, ?, ?, ?, ?)")
                .bind(work_id)
                .bind(pseud)
                .bind("A Work For Resonance")
                .bind(now)
                .bind(now)
                .execute(tdb.db().sqlite_pool().expect("sqlite"))
                .await
                .expect("work row");
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at) \
                 VALUES ($1::uuid, $2::uuid, $3, $4, $5)",
            )
            .bind(work_id)
            .bind(pseud)
            .bind("A Work For Resonance")
            .bind(now)
            .bind(now)
            .execute(tdb.db().postgres_pool().expect("postgres"))
            .await
            .expect("work row");
        }
    }
    // The owner is read from `work_contributors` by `collaboration::owner_of`, which is where a published work keeps
    // it — `works.owner_pseud_id` alone is not the attribution table, and a fixture that writes only the work tests a
    // door against a row shape the product never produces.
    match tdb.db().backend() {
        Backend::Sqlite => {
            sqlx::query("INSERT INTO work_contributors (work_id, pseud_id, role, public_attribution, created_at) VALUES (?, ?, 'owner', 1, ?)")
                .bind(work_id)
                .bind(pseud)
                .bind(now)
                .execute(tdb.db().sqlite_pool().expect("sqlite"))
                .await
                .expect("owner row");
        }
        Backend::Postgres => {
            sqlx::query("INSERT INTO work_contributors (work_id, pseud_id, role, public_attribution, created_at) VALUES ($1::uuid, $2::uuid, 'owner', 1, $3)")
                .bind(work_id)
                .bind(pseud)
                .bind(now)
                .execute(tdb.db().postgres_pool().expect("postgres"))
                .await
                .expect("owner row");
        }
    }
}

fn build(config: &Config, db: &Database) -> axum::Router {
    server::build_router(AppState::new(config.clone(), db.clone()))
}

#[tokio::test]
async fn the_window_opens_once_labels_need_it_closed_and_only_the_owner_reads_theirs() {
    let dir = scratch_dir("leakage");
    let tdb = TestDb::connect_with_dir("m7013-leakage", &dir).await;
    let db: Database = tdb.db().clone();

    let mut config = Config::development_defaults();
    config.storage.root = dir.clone();

    // The writer, whose work will carry the label.
    let mut writer = TestClient::new(build(&config, &db));
    let account = register(&mut writer, "leakage-writer@example.test", "leakagewriter").await;
    let pseud = active_pseud_id(&mut writer, &account)
        .await
        .expect("active pseud");
    let work = id("leakage-work");
    insert_work(&tdb, &work, &pseud).await;

    // The operator: a separate account, signed in through a build that knows it is the operator.
    let mut op_probe = TestClient::new(build(&config, &db));
    let op_account = register(
        &mut op_probe,
        "leakage-operator@example.test",
        "leakageoperator",
    )
    .await;
    let mut op_config = config.clone();
    op_config.administration.operator_account_id = Some(op_account.parse().expect("account id"));
    let mut operator = TestClient::new(build(&op_config, &db));
    sign_in_as(
        &mut operator,
        &tdb,
        "leakage-operator@example.test",
        "leakageoperator",
    )
    .await;

    // 1. A fresh instance has never batched, and that is a state, not a fault.
    let (status, body) = operator.get("/api/v1/admin/leakage/window").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["window"].is_null(), "{body}");

    // 2. A label cannot exist before a window has closed: its staleness would be undecidable.
    let (status, body) = operator
        .put(
            format!("/api/v1/admin/works/{work}/resonance"),
            serde_json::json!({"label": "noticed"}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["error"]["code"],
        serde_json::json!("VALIDATION_FAILED"),
        "{body}"
    );

    // 3. Open a window. The answer is the window, with `open: true` and no close.
    let (status, window) = operator
        .post("/api/v1/admin/leakage/window", serde_json::json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "{window}");
    assert_eq!(window["open"], serde_json::json!(true), "{window}");
    assert!(window["closed_at"].is_null(), "{window}");
    let window_id = window["id"].as_str().expect("window id").to_owned();

    let (_, latest) = operator.get("/api/v1/admin/leakage/window").await;
    assert_eq!(
        latest["window"]["id"],
        serde_json::json!(window_id),
        "{latest}"
    );

    // 4. A second open is refused: two live windows would make a payout attributable to either, which is the
    //    correlation §52.2 exists to prevent.
    let (status, body) = operator
        .post("/api/v1/admin/leakage/window", serde_json::json!({}))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("already open")),
        "{body}"
    );

    // 5. A label still cannot be written while the window is open.
    let (status, body) = operator
        .put(
            format!("/api/v1/admin/works/{work}/resonance"),
            serde_json::json!({"label": "noticed"}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    // 6. Close it, and the close is stamped once.
    let (status, closed) = operator
        .post(
            format!("/api/v1/admin/leakage/window/{window_id}/close"),
            serde_json::json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    assert_eq!(closed["open"], serde_json::json!(false), "{closed}");
    assert!(closed["closed_at"].as_i64().is_some(), "{closed}");
    let (status, body) = operator
        .post(
            format!("/api/v1/admin/leakage/window/{window_id}/close"),
            serde_json::json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    // 7. Now a label. The owner comes from the work, not from the body.
    let (status, label) = operator
        .put(
            format!("/api/v1/admin/works/{work}/resonance"),
            serde_json::json!({"label": "noticed"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{label}");
    assert_eq!(label["label"], serde_json::json!("noticed"), "{label}");
    assert_eq!(label["window_id"], serde_json::json!(window_id), "{label}");

    // 8. The owner reads it back; a stranger gets a 404, not a 403.
    let (status, mine) = writer.get(format!("/api/v1/works/{work}/resonance")).await;
    assert_eq!(status, StatusCode::OK, "{mine}");
    assert_eq!(mine["label"], serde_json::json!("noticed"), "{mine}");

    let mut stranger = TestClient::new(build(&config, &db));
    register(
        &mut stranger,
        "leakage-stranger@example.test",
        "leakagestranger",
    )
    .await;
    let (status, body) = stranger
        .get(format!("/api/v1/works/{work}/resonance"))
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a stranger must not learn this: {body}"
    );
    assert!(
        body["label"].is_null(),
        "and no label leaks in the refusal: {body}"
    );

    // 9. A label that is not one of the four buckets is refused, and the refusal names them.
    let (status, body) = operator
        .put(
            format!("/api/v1/admin/works/{work}/resonance"),
            serde_json::json!({"label": "very popular"}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"]["field_errors"]["label"]
            .as_str()
            .is_some_and(|m| m.contains("landing")),
        "{body}"
    );

    // 10. The writer is not an operator: the window doors do not exist for them.
    let (status, body) = writer
        .post("/api/v1/admin/leakage/window", serde_json::json!({}))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}
