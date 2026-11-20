//! TA1 — private taste candidates come from stored metadata on both engines.

use darkhold_db::taste_curation::candidates;
use darkhold_db::{Backend, Database, DatabaseConfig};
use std::path::PathBuf;

const ACCOUNT: &str = "01956c00-0000-4000-8000-000000000001";
const OTHER: &str = "01956c00-0000-4000-8000-000000000009";
const ITEM: &str = "01956c00-0000-4000-8000-000000000011";
const ITEM_OTHER: &str = "01956c00-0000-4000-8000-000000000012";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-taste-curation-{tag}-{}-{:?}",
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
                "lh_test_tastecur_{}_{}",
                std::process::id(),
                COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            );
            test_support::empty_postgres(&admin_url, &name)
                .await
                .expect("an empty PostgreSQL database")
        }
        _ => {
            let config =
                DatabaseConfig::new(format!("sqlite://{}/tastecur.sqlite?mode=rwc", dir.display()));
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
    for (id, email, _handle) in [
        (ACCOUNT, "tastecur@example.invalid", "tastecur"),
        (OTHER, "tastecur2@example.invalid", "tastecur2"),
    ] {
        exec(
            db,
            &format!(
                "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version)
                 VALUES ('{id}','active','{email}','adult',
                         '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
            ),
        )
        .await;
    }
    exec(
        db,
        &format!(
            "INSERT INTO library_items (id, account_id, source_key, source_work_key, title,
                                        author_text, summary, source_url, provenance_json,
                                        created_at, updated_at, version)
             VALUES ('{ITEM}','{ACCOUNT}','ao3','w1','Dark MC With A System','author',
                     'Progression fantasy','https://example.invalid/1',
                     '{{\"tags\":[\"Dark MC\",\"Progression Fantasy\"]}}',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO library_items (id, account_id, source_key, source_work_key, title,
                                        author_text, summary, source_url, provenance_json,
                                        created_at, updated_at, version)
             VALUES ('{ITEM_OTHER}','{OTHER}','ao3','w2','Not Mine','author',
                     'summary','https://example.invalid/2','{{}}',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;
}

#[tokio::test]
async fn candidates_are_scoped_to_the_account_and_limit() {
    let (db, _dir) = database("candidates").await;
    seed(&db).await;
    let rows = candidates(&db, ACCOUNT, 10).await.expect("candidates");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].library_item_id, ITEM);
    assert!(rows[0].provenance_json.contains("Dark MC"));
    assert!(candidates(&db, ACCOUNT, 0)
        .await
        .expect("empty limit")
        .is_empty());
}
