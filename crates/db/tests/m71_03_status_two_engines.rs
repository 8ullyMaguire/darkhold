//! M71-03: a search hit carries the work's lifecycle status, from one grouped query on both engines.
//!
//! Runs on whichever engine `DARKHOLD_TEST_PG_URL` names: SQLite when unset, PostgreSQL when set.

use darkhold_db::{
    search::search_works_ast_scoped, search::SearchFilters, Backend, Database, DatabaseConfig,
};

const ACCOUNT: &str = "01955300-0000-4000-8000-000000ea0001";
const PSEUD: &str = "01955300-0000-4000-8000-000000eb0001";
const LIVE: &str = "01955300-0000-4000-8000-000000ec0101";
const HELD: &str = "01955300-0000-4000-8000-000000ec0102";

fn config(url: String) -> DatabaseConfig {
    let mut c = DatabaseConfig::new(url);
    c.max_connections = 2;
    c.acquire_timeout = std::time::Duration::from_secs(60);
    c
}

async fn connect() -> Database {
    let Ok(admin) = std::env::var("DARKHOLD_TEST_PG_URL") else {
        let mut dir = std::env::temp_dir();
        dir.push(format!("darkhold-status-{}", uuid::Uuid::new_v4().simple()));
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
    let name = format!("lh_status_{}", uuid::Uuid::new_v4().simple());
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

async fn fixture(db: &Database) {
    exec(
        db,
        &format!(
            "INSERT INTO accounts (id, email, created_at, updated_at)
             VALUES ('{ACCOUNT}', 'status@example.invalid', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at)
             VALUES ('{PSEUD}', '{ACCOUNT}', 'statusowner', 'Status Owner', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        ),
    )
    .await;
    for (id, lifecycle, visibility) in [(LIVE, "published", "public"), (HELD, "draft", "private")] {
        exec(
            db,
            &format!(
                "INSERT INTO works (id, owner_pseud_id, title, lifecycle, visibility, created_at, updated_at)
                 VALUES ('{id}', '{PSEUD}', 'Status probe {id}', '{lifecycle}', '{visibility}',
                         '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
            ),
        )
        .await;
        exec(
            db,
            &format!(
                "INSERT INTO works_index_terms (work_id, term, pos) VALUES ('{id}', 'probe', 0)"
            ),
        )
        .await;
    }
}

#[tokio::test]
async fn search_results_carry_lifecycle_status_on_both_engines() {
    let db = connect().await;
    fixture(&db).await;
    let results = search_works_ast_scoped(
        &db,
        "probe",
        Some(ACCOUNT),
        20,
        &[],
        &SearchFilters::default(),
    )
    .await
    .expect("status search");
    assert!(
        results.len() >= 2,
        "both probe works should be visible to their owner"
    );
    let live = results
        .iter()
        .find(|r| r.work_id == LIVE)
        .expect("live work");
    let held = results
        .iter()
        .find(|r| r.work_id == HELD)
        .expect("held work");
    assert_eq!(live.status, "live");
    assert_eq!(held.status, "held");
}
