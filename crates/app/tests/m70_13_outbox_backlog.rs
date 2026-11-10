//! How many events have not been delivered (M70-13 pass 58).
//!
//! `outbox::undelivered_count` existed and had no caller. The outbox is the instance's delivery spine — likes, follows,
//! publications, webhook fan-out all go through it — and the one question an operator asks when something "did not arrive" is
//! exactly the question this function answers, and nothing asked it. `/health/ready` reported the database, the migrations and
//! the storage root; the queue that carries every side effect was invisible.
//!
//! **It is a check, not a gate.** Readiness answers "should this process receive traffic", and a delivery backlog is not a
//! reason to be drained out of a load balancer or restarted — that would turn a slow webhook into an outage. So `ok` here
//! reports only whether the count could be *read*: a query that fails is a real problem (the outbox table is gone, or the
//! schema moved), while a large number is an observation a monitor alerts on. The count is in the `detail`, so a monitor reads
//! it without a second endpoint and without a second query implementation.
//!
//! Not answered here, and named as a follow-up: **the age of the oldest undelivered event**. A count cannot tell "five events
//! just enqueued" from "five events stuck since yesterday", and the second is the failure an operator cares about. No query for
//! it exists yet.

use axum::http::StatusCode;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::Database;
use serde_json::Value;
use test_support::{scratch_dir, TestClient, TestDb};

fn build(config: &Config, db: &Database) -> axum::Router {
    server::build_router(AppState::new(config.clone(), db.clone()))
}

/// Run one statement with the right placeholder dialect.
async fn exec(db: &Database, sqlite: &str, postgres: &str) {
    let sql = db.sql(sqlite, postgres);
    match db.backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(&sql)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("exec");
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(&sql)
                .execute(db.postgres_pool().expect("postgres"))
                .await
                .expect("exec");
        }
    }
}

/// The `outbox` check's detail string from `/health/ready`.
async fn outbox_detail(client: &mut TestClient) -> (StatusCode, Value, String) {
    let (status, body) = client.get("/health/ready").await;
    let check = body["checks"]["outbox"].clone();
    let detail = check["detail"].as_str().unwrap_or_default().to_owned();
    (status, check, detail)
}

#[tokio::test]
async fn readiness_reports_the_undelivered_backlog_and_never_gates_on_it() {
    let dir = scratch_dir("outbox-backlog");
    let tdb = TestDb::connect_with_dir("m7013-outbox-backlog", &dir).await;
    let db: Database = tdb.db().clone();
    let config = Config::development_defaults();
    let mut client = TestClient::new(build(&config, &db));

    // 1. A quiet instance reports zero, and readiness still passes. The check must be present at all — its absence was the
    //    defect: three dependencies were watched and the delivery spine was not.
    let (status, check, detail) = outbox_detail(&mut client).await;
    assert_eq!(status, StatusCode::OK, "a quiet instance is ready");
    assert_eq!(
        check["ok"],
        serde_json::json!(true),
        "the check must exist and pass: {check}"
    );
    assert!(
        detail.contains('0'),
        "an empty outbox reports zero undelivered: {detail}"
    );

    // 2. Three events, and the count moves — this is what proves the number is read from the table rather than reported as a
    //    constant, and it is the assertion a hard-coded `\"0 undelivered\"` fails.
    // The dedupe key is a uuid, not a fixed string: on PostgreSQL the harness's tables persist between runs, and a fixed key
    // makes the second run insert nothing (`ON CONFLICT DO NOTHING`) — so the count would read 0 for a reason that has
    // nothing to do with the code under test. A test that only passes on a clean database is a test that fails on a second
    // run, which is the run nobody watches.
    let run = uuid::Uuid::new_v4();
    for index in 0..3 {
        darkhold_db::outbox::enqueue(
            &db,
            "work.published",
            &format!("{{\"n\":{index}}}"),
            Some(&format!("backlog-{run}-{index}")),
        )
        .await
        .expect("enqueue");
    }
    let (status, check, detail) = outbox_detail(&mut client).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a backlog does not make an instance unready"
    );
    assert_eq!(check["ok"], serde_json::json!(true), "{check}");
    assert!(
        detail.contains('3'),
        "three undelivered events are counted: {detail}"
    );

    // 3. Delivered means delivered: marking one removes it from the count. `undelivered_count` filters on `delivered_at IS
    //    NULL`, so a delivered row is not "delivered but still counted" — the boundary a queue's counter most often gets wrong.
    let pending = darkhold_db::outbox::pending(&db, 10)
        .await
        .expect("pending");
    assert_eq!(pending.len(), 3, "three events are waiting: {pending:?}");
    darkhold_db::outbox::mark_delivered(
        &db,
        darkhold_domain::OutboxEventId::from_uuid(
            uuid::Uuid::parse_str(&pending[0].id).expect("event id"),
        ),
    )
    .await
    .expect("deliver");
    let (status, check, detail) = outbox_detail(&mut client).await;
    assert_eq!(status, StatusCode::OK, "{check}");
    assert!(
        detail.contains('2'),
        "a delivered event is no longer undelivered: {detail}"
    );
    assert!(
        !detail.contains('3'),
        "the count is not cumulative: {detail}"
    );

    // 4. The age of the oldest undelivered event, which the count cannot supply. One of the two remaining events is backdated
    //    three days: a frozen queue and a busy one both read "2 undelivered", and only the age tells them apart. This is the
    //    assertion the deployed instance needs — its outbox holds tens of thousands of `work.published` rows held back for a
    //    disabled bridge, so a bare count is permanently large and says nothing about health.
    exec(
        &db,
        "UPDATE outbox_events SET created_at = datetime('now', '-3 days') WHERE delivered_at IS NULL",
        "UPDATE outbox_events SET created_at = to_char(now() AT TIME ZONE 'UTC' - interval '3 days', 'YYYY-MM-DD HH24:MI:SS') WHERE delivered_at IS NULL",
    )
    .await;
    let (status, check, detail) = outbox_detail(&mut client).await;
    assert_eq!(status, StatusCode::OK, "{check}");
    assert!(
        detail.contains("3d"),
        "the oldest undelivered event's age is reported: {detail}"
    );
    assert!(
        detail.contains("2 undelivered"),
        "the age is reported beside the count: {detail}"
    );
}
