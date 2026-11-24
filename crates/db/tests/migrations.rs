//! M45-76 Step 1: migration 0218 must put completion provenance in the schema.
//!
//! The probe uses the real embedded catalogue through `TestDb`, not a hand-run
//! SQL file: an operator upgrades by replacing one binary, so a migration that
//! is on disk but not in the catalogue does not exist for this test.

use darkhold_db::Backend;
use sqlx::Row;
use std::path::PathBuf;

fn scratch(tag: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("darkhold-m45-76-0218-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

async fn has_column(db: &darkhold_db::Database, table: &str, column: &str) -> bool {
    let (sql, bind) = match db.backend() {
        Backend::Sqlite => (
            "SELECT count(*) FROM pragma_table_info(?) WHERE name = ?",
            None,
        ),
        Backend::Postgres => (
            "SELECT count(*) FROM information_schema.columns \
             WHERE table_name = $1 AND column_name = $2",
            Some(()),
        ),
    };
    let count: i64 = match (db.backend(), bind) {
        (Backend::Sqlite, _) => sqlx::query(sql)
            .bind(table)
            .bind(column)
            .fetch_one(db.sqlite_pool().expect("sqlite handle"))
            .await
            .expect("probe sqlite column")
            .get(0),
        (Backend::Postgres, _) => sqlx::query(sql)
            .bind(table)
            .bind(column)
            .fetch_one(db.postgres_pool().expect("postgres handle"))
            .await
            .expect("probe postgres column")
            .get(0),
    };
    count > 0
}

async fn has_table(db: &darkhold_db::Database, table: &str) -> bool {
    let count: i64 = match db.backend() {
        Backend::Sqlite => {
            sqlx::query("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?")
                .bind(table)
                .fetch_one(db.sqlite_pool().expect("sqlite handle"))
                .await
                .expect("probe sqlite table")
                .get(0)
        }
        Backend::Postgres => sqlx::query(
            "SELECT count(*) FROM information_schema.tables \
             WHERE table_name = $1 AND table_schema = 'public'",
        )
        .bind(table)
        .fetch_one(db.postgres_pool().expect("postgres handle"))
        .await
        .expect("probe postgres table")
        .get(0),
    };
    count > 0
}

#[tokio::test]
async fn migration_0218_adds_completion_provenance() {
    let tdb = test_support::TestDb::connect_with_dir("m45-76-0218", &scratch("schema")).await;
    let db = tdb.db();
    assert!(
        has_column(db, "works", "completion_source").await,
        "works.completion_source is missing"
    );
    assert!(
        has_column(db, "works", "completion_marked_at").await,
        "works.completion_marked_at is missing"
    );
    assert!(
        has_table(db, "work_completion_marks").await,
        "work_completion_marks is missing"
    );
    assert!(
        tdb.applied_migrations()
            .iter()
            .any(|id| id == "0218_work_completion_marks"),
        "the embedded catalogue must record 0218 as applied: {:?}",
        tdb.applied_migrations()
    );
    tdb.cleanup().await;
}
