//! M45-76 Step 4.1: the app sweep consumes candidates, the pure decision and
//! the conditional transition. A dry run counts without writing; an apply run
//! writes once and the next page sees nothing left.

use darkhold_app::config::Config;
use darkhold_app::state::AppState;
use darkhold_app::work_lifecycle::sweep_page;
use darkhold_db::DatabaseConfig;
use std::path::PathBuf;

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("darkhold-sweep-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

async fn exec(db: &darkhold_db::Database, sql: &str) {
    sqlx::query(sql)
        .execute(db.sqlite_pool().expect("sqlite handle"))
        .await
        .expect("fixture");
}

#[tokio::test]
async fn sweep_dry_run_does_not_write_and_apply_writes_once() {
    let dir = scratch("one-page");
    let tdb = test_support::TestDb::connect_with_dir("sweep-one-page", &dir).await;
    let db = tdb.db();
    exec(db, "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version) VALUES ('01957600-0000-4000-8000-00000000d001','active','sweep@test.dev','adult','2001-01-01T00:00:00Z','2001-01-01T00:00:00Z',1)").await;
    exec(db, "INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at, updated_at, version) VALUES ('01957600-0000-4000-8000-00000000d002','01957600-0000-4000-8000-00000000d001','sweep','Sweep','listed','2001-01-01T00:00:00Z','2001-01-01T00:00:00Z',1)").await;
    exec(db, "INSERT INTO works (id, owner_pseud_id, title, visibility, lifecycle, completion, created_at, updated_at) VALUES ('01957600-0000-4000-8000-00000000d003','01957600-0000-4000-8000-00000000d002','Stale','public','published','in_progress','2001-01-01T00:00:00Z','2001-01-01T00:00:00Z')").await;
    exec(db, "INSERT INTO library_items (id, account_id, work_id, source_key, source_work_key, title, status, source_updated_at, source_url, created_at, updated_at) VALUES ('01957600-0000-4000-8000-00000000d004','01957600-0000-4000-8000-00000000d001','01957600-0000-4000-8000-00000000d003','sweep','w1','Stale','ongoing','2001-01-01T00:00:00Z','https://example.invalid/1','2001-01-01T00:00:00Z','2001-01-01T00:00:00Z')").await;

    let mut config = Config::development_defaults();
    config.works.auto_abandon_after_months = 4;
    config.database = DatabaseConfig::new(format!(
        "sqlite://{}/darkhold.sqlite?mode=rwc",
        dir.display()
    ));
    let state = AppState::new(config, db.clone());

    let dry = sweep_page(&state, None, 10, true).await.expect("dry run");
    assert!(dry.dry_run);
    assert_eq!(dry.scanned, 1);
    assert_eq!(dry.abandoned, 1, "a 25-year-old source clock is stale");
    let completion: String = sqlx::query_scalar(
        "SELECT completion FROM works WHERE id = '01957600-0000-4000-8000-00000000d003'",
    )
    .fetch_one(db.sqlite_pool().expect("sqlite handle"))
    .await
    .expect("read after dry run");
    assert_eq!(completion, "in_progress", "a dry run must not write");

    let applied = sweep_page(&state, None, 10, false)
        .await
        .expect("apply run");
    assert_eq!(applied.abandoned, 1);
    let completion: String = sqlx::query_scalar(
        "SELECT completion FROM works WHERE id = '01957600-0000-4000-8000-00000000d003'",
    )
    .fetch_one(db.sqlite_pool().expect("sqlite handle"))
    .await
    .expect("read after apply");
    assert_eq!(completion, "abandoned");
    let marks: i64 = sqlx::query("SELECT source, observed_last_activity FROM work_completion_marks")
        .fetch_all(db.sqlite_pool().expect("sqlite handle"))
        .await
        .expect("read marks")
        .len() as i64;
    assert_eq!(marks, 1);

    let again = sweep_page(&state, None, 10, false)
        .await
        .expect("second apply");
    // Abandoned works stay in the candidate queue: a later story update must
    // be able to revive them. The second page simply has nothing to change.
    assert_eq!(again.scanned, 1);
    assert!(again.done);
    assert_eq!(again.abandoned, 0);
    assert_eq!(again.raced, 0);

    tdb.cleanup().await;
}
