//! A work card whose counters drifted, and the repair nobody could run (M70-13 pass 59).
//!
//! `work_metrics::recompute_and_store` is documented as "used by the seed script and as a repair tool" and had no caller in the
//! binary. The counters on a work card are maintained **incrementally** — `increment_views`, `toggle_kudos`, `increment_counter`
//! at the moment an action happens — and the module's header states why ("never by a trigger", so the dialect-parity test stays
//! green). Incremental maintenance has one failure mode a materialized aggregate cannot avoid: **drift**. A request that dies
//! between the event write and the counter bump, an import that writes events directly, a bug fixed three months ago — all of
//! them leave the stored number permanently wrong, and the only repair was `psql`.
//!
//! `POST /admin/works/{id}/metrics/recompute` (operator) is that repair. It answers with the row it just stored, so the
//! operator sees the numbers a reader will see rather than a description of what happened.

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

/// A work owned by a fresh pseud, plus `views` distinct view events.
async fn work_with_views(db: &Database, email: &str, handle: &str, views: i64) -> String {
    let mut client = TestClient::new(build(&Config::development_defaults(), db));
    let account = register(&mut client, email, handle).await;
    let pseud = uuid::Uuid::new_v4().to_string();
    exec(
        db,
        &format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at) \
             VALUES ('{pseud}', '{account}', '{handle}-pseud', '{handle}', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        ),
        &format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at) \
             VALUES ('{pseud}', '{account}'::uuid, '{handle}-pseud', '{handle}', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        ),
    )
    .await;
    let work = uuid::Uuid::new_v4().to_string();
    exec(
        db,
        &format!(
            "INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at) \
             VALUES ('{work}', '{pseud}', 'Counted Work', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        ),
        &format!(
            "INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at) \
             VALUES ('{work}'::uuid, '{pseud}'::uuid, 'Counted Work', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        ),
    )
    .await;
    for index in 0..views {
        exec(
            db,
            &format!(
                "INSERT INTO work_view_log (work_id, viewer_hash, viewed_at, is_automated) \
                 VALUES ('{work}', 'viewer-{index}', '2026-01-0{}-00:00:00Z', 0)",
                index + 1
            ),
            &format!(
                "INSERT INTO work_view_log (work_id, viewer_hash, viewed_at, is_automated) \
                 VALUES ('{work}'::uuid, 'viewer-{index}', '2026-01-0{}-00:00:00Z', 0)",
                index + 1
            ),
        )
        .await;
    }
    work
}

/// The stored `updated_at` for a work, as text.
async fn updated_at(db: &Database, work: &str) -> String {
    match db.backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query_scalar("SELECT updated_at FROM work_metric_aggregates WHERE work_id = ?")
                .bind(work)
                .fetch_one(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("stamp")
        }
        darkhold_db::Backend::Postgres => sqlx::query_scalar(
            "SELECT updated_at FROM work_metric_aggregates WHERE work_id::text = $1",
        )
        .bind(work)
        .fetch_one(db.postgres_pool().expect("postgres"))
        .await
        .expect("stamp"),
    }
}

/// `YYYY-MM-DD HH:MM:SS` — what SQLite's `datetime('now')` and the project's PG `to_char(... AT TIME ZONE 'UTC', ...)` both
/// produce. Anything with a `+`, a `T` or a fraction is a different shape from the rest of the column.
fn is_schema_timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 19
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b' '
        && bytes[13] == b':'
        && bytes[16] == b':'
}

#[tokio::test]
async fn an_operator_repairs_a_drifted_counter_and_the_repair_is_what_readers_see() {
    let dir = scratch_dir("recompute-metrics");
    let tdb = TestDb::connect_with_dir("m7013-recompute-metrics", &dir).await;
    let db: Database = tdb.db().clone();

    let work = work_with_views(&db, "rm-author@example.test", "rmauthor", 2).await;

    // The drift: a stored row claiming 999 views while two view events exist. This is not a hypothetical — it is what a request
    // that died between the event write and the counter bump leaves behind, and it is what `get_metrics` will serve until
    // somebody repairs it.
    exec(
        &db,
        &format!(
            "INSERT INTO work_metric_aggregates (work_id, views, complete_reads, reactions, kudos, bookmarks, collection_adds, reviews, updated_at) \
             VALUES ('{work}', 999, 0, 0, 0, 0, 0, 0, '2026-01-01T00:00:00Z')"
        ),
        &format!(
            "INSERT INTO work_metric_aggregates (work_id, views, complete_reads, reactions, kudos, bookmarks, collection_adds, reviews, updated_at) \
             VALUES ('{work}'::uuid, 999, 0, 0, 0, 0, 0, 0, '2026-01-01T00:00:00Z')"
        ),
    )
    .await;
    let before = darkhold_db::work_metrics::get_metrics(
        &db,
        &work.parse::<darkhold_domain::WorkId>().expect("work id"),
    )
    .await
    .expect("metrics");
    assert_eq!(before.views, 999, "the drifted row is what readers see");

    // An operator, named by configuration — this instance's break-glass identity, and the same door every other admin route uses.
    let mut operator = TestClient::new(build(&Config::development_defaults(), &db));
    let account = register(&mut operator, "rm-operator@example.test", "rmoperator").await;
    let mut config = Config::development_defaults();
    config.administration.operator_account_id = Some(account.parse().expect("account id"));
    let mut operator = TestClient::new(build(&config, &db));
    test_support::sign_in_as(
        &mut operator,
        &tdb,
        "rm-operator@example.test",
        "rmoperator",
    )
    .await;

    let (status, body) = operator
        .post(
            &format!("/api/v1/admin/works/{work}/metrics/recompute"),
            json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["views"],
        json!(2),
        "the answer is the recomputed row: {body}"
    );

    // Stored, not merely returned: this is the assertion that separates a repair from a preview, and the one an implementation
    // that computed the numbers for the response without writing them fails.
    let after = darkhold_db::work_metrics::get_metrics(
        &db,
        &work.parse::<darkhold_domain::WorkId>().expect("work id"),
    )
    .await
    .expect("metrics");
    assert_eq!(after.views, 2, "the repair is durable: {after:?}");

    // ...and it is stamped in the shape the rest of the schema stores (M70-13 pass 59). `updated_at` is TEXT on both engines,
    // so a recompute that writes `now()` on PostgreSQL stores `2026-11-09 12:34:56+01` — an offset that no other row has, and
    // that sorts *after* every correctly shaped timestamp at the same second because '+' beats nothing. The value is readable,
    // so nothing fails loudly; the damage is a column whose ordering silently lies.
    let stamp = updated_at(&db, &work).await;
    assert!(
        is_schema_timestamp(&stamp),
        "updated_at must be UTC with no offset, like every other timestamp in this schema: {stamp:?}"
    );

    // A caller with no standing gets `404`, not `403` (ADR 0030): the surface's existence is not theirs to learn.
    let mut stranger = TestClient::new(build(&Config::development_defaults(), &db));
    test_support::sign_in_as(&mut stranger, &tdb, "rm-author@example.test", "rmauthor").await;
    let (status, body) = stranger
        .post(
            &format!("/api/v1/admin/works/{work}/metrics/recompute"),
            json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}
