//! A claim that was never fulfilled, and the sweep nothing ran (M70-13).
//!
//! `events::expire_claims` carries the sentence that is the whole finding: *"Called by the job queue."* No queue called it, no
//! route called it, no test called it — so a reader who claimed a request and then stopped writing left the claim `outstanding`
//! forever. A claim is what tells the next reader "somebody is already on this"; one that never expires is a request nobody can
//! pick up, and the failure is invisible because the row is doing exactly what it says.
//!
//! The store half is careful and already correct: it keys on `status = 'outstanding'` rather than on `fulfilled_at IS NULL`, so a
//! second run finds nothing rather than re-expiring the same rows forever. Migration 0080 added that column for precisely this.
//!
//! A store function with no scheduler is the same defect as a table with no writer. The fix is one call in the worker's
//! maintenance pass, beside the other sweeps nobody presses a button for.

use darkhold_app::worker::{Worker, WorkerOptions};
use test_support::{scratch_dir, TestDb};

async fn state_for(tdb: &TestDb, dir: &std::path::Path) -> darkhold_app::state::AppState {
    let mut config = darkhold_app::config::Config::development_defaults();
    config.storage.root = dir.to_path_buf();
    darkhold_app::state::AppState::new(config, tdb.db().clone())
}

async fn exec(db: &darkhold_db::Database, statement: &str) {
    match db.backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(statement)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await
                .unwrap_or_else(|error| panic!("{statement}\n{error}"));
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(statement)
                .execute(db.postgres_pool().expect("postgres"))
                .await
                .unwrap_or_else(|error| panic!("{statement}\n{error}"));
        }
    }
}

async fn claim_status(db: &darkhold_db::Database, request: &str) -> String {
    let sql = match db.backend() {
        darkhold_db::Backend::Sqlite => "SELECT status FROM claims WHERE request_id = ?",
        darkhold_db::Backend::Postgres => "SELECT status FROM claims WHERE request_id = $1",
    };
    match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar(sql)
            .bind(request)
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("the claim exists"),
        darkhold_db::Backend::Postgres => sqlx::query_scalar(sql)
            .bind(request)
            .fetch_one(db.postgres_pool().expect("postgres"))
            .await
            .expect("the claim exists"),
    }
}

/// One outstanding claim, at `claimed_at`. No `requests` row: the claim table carries `request_id` as TEXT with no foreign key
/// (0014), so a claim can outlive a deleted request — which is itself a reason the sweep must run.
async fn claimed(db: &darkhold_db::Database, request: &str, claimed_at: &str) {
    exec(
        db,
        &format!(
            "INSERT INTO claims (request_id, claimant, claimed_at, fulfilled_by_work, fulfilled_at, status) \
             VALUES ('{request}', 'pseud-{request}', '{claimed_at}', NULL, NULL, 'outstanding')"
        ),
    )
    .await;
}

#[tokio::test]
async fn a_maintenance_pass_expires_a_claim_nobody_fulfilled() {
    let dir = scratch_dir("claim-expiry");
    let tdb = TestDb::connect_with_dir("m7013-claim-expiry", &dir).await;
    let state = state_for(&tdb, &dir).await;
    let db = tdb.db();

    // The run-unique suffix: the PostgreSQL harness keeps tables between runs, and `claims`' primary key is
    // `(request_id, claimant)`, so a fixed id would collide on the second run and the insert would be the thing that failed.
    let run = uuid::Uuid::new_v4().simple().to_string();
    let stale = format!("request-stale-{run}");
    let fresh = format!("request-fresh-{run}");

    // 2026-01-01 is far past any grace period this binary can be configured with. The second claim is an hour old, not
    // brand new, and that detail is load-bearing: stamped with the worker's own clock it sits exactly on the cutoff, and the
    // store's comparison is strict (`claimed_at < cutoff`), so a mutant that set the grace to zero — the most obvious wrong
    // value — would leave a same-instant claim unexpired and pass. An hour of slack is what makes "recently claimed" a claim
    // the sweep must not touch, for any grace shorter than an hour.
    claimed(db, &stale, "2026-01-01T00:00:00Z").await;
    let an_hour_ago = (time::OffsetDateTime::now_utc() - time::Duration::hours(1))
        .format(&time::format_description::well_known::Rfc3339)
        .expect("format an hour ago");
    claimed(db, &fresh, &an_hour_ago).await;
    assert_eq!(claim_status(db, &stale).await, "outstanding");
    assert_eq!(claim_status(db, &fresh).await, "outstanding");

    Worker::new(WorkerOptions::named("claim-sweep"))
        .maintenance_pass(&state)
        .await
        .expect("maintenance pass");

    assert_eq!(
        claim_status(db, &stale).await,
        "expired",
        "a claim nobody fulfilled is what the sweep is for"
    );
    assert_eq!(
        claim_status(db, &fresh).await,
        "outstanding",
        "a claim made a moment ago is somebody working on it, not an abandoned one"
    );

    // Idempotent: the predicate is `status = 'outstanding'`, so the second pass matches nothing. A sweep that re-stamped the
    // rows it had already expired would make its own count meaningless — which is what the old `fulfilled_at = NULL` form did.
    Worker::new(WorkerOptions::named("claim-sweep"))
        .maintenance_pass(&state)
        .await
        .expect("second maintenance pass");
    assert_eq!(claim_status(db, &stale).await, "expired");
    assert_eq!(claim_status(db, &fresh).await, "outstanding");
}
