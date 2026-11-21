//! TA3-2 — several workers may run, but no archive gets more than one leased import.
//!
//! The owner authorized extra workers only under that constraint: parallelism is
//! for the queue, not for hammering one source site.

use darkhold_db::jobs;
use darkhold_db::{imports, Backend, Database, DatabaseConfig};
use darkhold_domain::jobs::{JobKind, RetryPolicy};
use std::path::PathBuf;
use time::OffsetDateTime;

const ACCOUNT: &str = "01956c00-0000-4000-8000-000000000011";
const PSEUD: &str = "01956c00-0000-4000-8000-000000000012";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-per-source-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

async fn database(tag: &str) -> (Database, PathBuf) {
    let dir = scratch_dir(tag);
    let db = match std::env::var("DARKHOLD_TEST_PG_URL") {
        Ok(admin_url) if !admin_url.is_empty() => {
            static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let name = format!(
                "lh_test_persource_{}_{}",
                std::process::id(),
                COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            );
            test_support::empty_postgres(&admin_url, &name)
                .await
                .expect("an empty PostgreSQL database")
        }
        _ => {
            let config = DatabaseConfig::new(format!(
                "sqlite://{}/persource.sqlite?mode=rwc",
                dir.display()
            ));
            Database::connect(&config).await.expect("connect")
        }
    };
    db.migrate().await.expect("migrate");
    (db, dir)
}

async fn exec(db: &Database, statement: &str) {
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(statement)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await
                .unwrap_or_else(|error| panic!("{statement}\n{error}"));
        }
        Backend::Postgres => {
            sqlx::query(statement)
                .execute(db.postgres_pool().expect("postgres"))
                .await
                .unwrap_or_else(|error| panic!("{statement}\n{error}"));
        }
    }
}

async fn seed(db: &Database) {
    exec(
        db,
        &format!(
            "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version)
             VALUES ('{ACCOUNT}','active','persource@example.invalid','adult',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at,
                                 updated_at, version)
             VALUES ('{PSEUD}','{ACCOUNT}','persource','Per Source','listed',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;
}

async fn enqueue_import(db: &Database, source_key: &str) -> String {
    let queue_id = jobs::enqueue(
        db,
        JobKind::Import,
        r#"{"source_key":"probe"}"#,
        None,
        None,
        0,
        &RetryPolicy::default(),
    )
    .await
    .expect("enqueue");
    let id = uuid::Uuid::new_v4().to_string();
    imports::create_import_job(
        db,
        &id,
        &queue_id.to_string(),
        ACCOUNT,
        PSEUD,
        source_key,
        &format!("https://example.invalid/{source_key}/{id}"),
        "library",
        false,
    )
    .await
    .expect("create import job");
    queue_id.to_string()
}

async fn scenario(tag: &str) {
    let (db, dir) = database(tag).await;
    seed(&db).await;
    let ao3_a = enqueue_import(&db, "ao3").await;
    let ao3_b = enqueue_import(&db, "ao3").await;
    let pawchive = enqueue_import(&db, "pawchive").await;
    let now = OffsetDateTime::now_utc();

    let first = jobs::claim_next(
        &db,
        "worker-a",
        std::time::Duration::from_secs(60),
        now,
        None,
        1,
        true,
        1,
    )
    .await
    .expect("claim")
    .expect("first import is claimable");
    assert_eq!(first.id, ao3_a);

    let second = jobs::claim_next(
        &db,
        "worker-b",
        std::time::Duration::from_secs(60),
        now,
        None,
        1,
        true,
        1,
    )
    .await
    .expect("claim");
    let other = second.expect("a different source is still claimable");
    assert_eq!(other.id, pawchive);

    let blocked = jobs::claim_next(
        &db,
        "worker-c",
        std::time::Duration::from_secs(60),
        now,
        None,
        1,
        true,
        1,
    )
    .await
    .expect("claim");
    assert!(
        blocked.is_none(),
        "the second ao3 import must stay blocked while the first is leased"
    );
    assert_ne!(other.id, ao3_b);


    drop(db);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn one_leased_import_per_source_sqlite() {
    scenario("sqlite").await;
}

#[tokio::test]
async fn one_leased_import_per_source_postgres() {
    if std::env::var("DARKHOLD_TEST_PG_URL")
        .map(|v| !v.is_empty())
        .unwrap_or(false)
    {
        scenario("postgres").await;
    }
}
