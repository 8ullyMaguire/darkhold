//! M71-08: resetting typography removes the account row on both engines, so the next render is the site default.
//!
//! Runs on whichever engine `DARKHOLD_TEST_PG_URL` names: SQLite when unset, PostgreSQL when set.

use darkhold_db::{
    reading::{reset_typography, save_typography, typography_for, TypographyInput},
    Backend, Database, DatabaseConfig,
};

const ACCOUNT: &str = "01955300-0000-4000-8000-000000f10001";

fn config(url: String) -> DatabaseConfig {
    let mut c = DatabaseConfig::new(url);
    c.max_connections = 2;
    c.acquire_timeout = std::time::Duration::from_secs(60);
    c
}

async fn connect() -> Database {
    let Ok(admin) = std::env::var("DARKHOLD_TEST_PG_URL") else {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "darkhold-typo-reset-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let db = Database::connect(&config(format!(
            "sqlite://{}/darkhold.sqlite?mode=rwc",
            dir.display()
        )))
        .await
        .expect("connect sqlite");
        db.migrate().await.expect("migrate");
        return db;
    };
    let name = format!("lh_typo_{}", uuid::Uuid::new_v4().simple());
    let admin_db = Database::connect(&config(admin.clone()))
        .await
        .expect("connect admin");
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(admin_db.postgres_pool().expect("postgres handle"))
        .await
        .expect("create database");
    let url = admin
        .rsplit_once('/')
        .map(|(prefix, _)| format!("{prefix}/{name}"))
        .expect("a database url has a path");
    let db = Database::connect(&config(url)).await.expect("connect");
    db.migrate().await.expect("migrate");
    db
}

async fn exec(db: &Database, sql: &str) {
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(sql)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await
                .expect("sqlite fixture");
        }
        Backend::Postgres => {
            sqlx::query(sql)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await
                .expect("postgres fixture");
        }
    }
}

#[tokio::test]
async fn resetting_typography_removes_the_account_row_on_both_engines() {
    let db = connect().await;
    exec(
        &db,
        &format!(
            "INSERT INTO accounts (id, status, email, created_at, updated_at)
             VALUES ('{ACCOUNT}', 'active', 'typo@example.test', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        ),
    )
    .await;

    let account = ACCOUNT.parse().expect("a valid account id");
    let saved = save_typography(
        &db,
        TypographyInput {
            account,
            expected_version: 0,
            font_scale: 1.4,
            line_height: 1.8,
            measure: 72,
            reader_theme: "dark",
            distraction_free: false,
        },
    )
    .await
    .expect("save");
    assert!(saved, "the first save creates the row");
    let before = typography_for(&db, account).await.expect("read before");
    assert_eq!(before.font_scale, 1.4, "the saved value is readable");
    assert!(before.version > 0, "a saved row has a version");

    reset_typography(&db, account)
        .await
        .expect("reset on this engine");
    let after = typography_for(&db, account).await.expect("read after");
    assert_eq!(after.font_scale, 1.0, "the site default comes back");
    assert_eq!(after.version, 0, "and the account row is gone");

    // A second reset is harmless: there is no row to delete.
    reset_typography(&db, account)
        .await
        .expect("second reset on this engine");
}
