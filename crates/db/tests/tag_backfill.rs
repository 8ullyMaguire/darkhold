//! M63-32 — source metadata already stored in `library_items.provenance_json`
//! can be backfilled into visible tag assertions without re-fetching a page.

use darkhold_db::tag_backfill::backfill_from_provenance;
use darkhold_db::{Backend, Database, DatabaseConfig};
use std::path::PathBuf;

const ACCOUNT: &str = "01956c00-0000-4000-8000-000000000001";
const PSEUD: &str = "01956c00-0000-4000-8000-000000000002";
const WORK_A: &str = "01956c00-0000-4000-8000-000000000003";
const WORK_B: &str = "01956c00-0000-4000-8000-000000000004";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-tag-backfill-{tag}-{}-{:?}",
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
                "lh_test_tagbf_{}_{}",
                std::process::id(),
                COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            );
            test_support::empty_postgres(&admin_url, &name)
                .await
                .expect("an empty PostgreSQL database")
        }
        _ => {
            let config =
                DatabaseConfig::new(format!("sqlite://{}/tagbf.sqlite?mode=rwc", dir.display()));
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

async fn count(db: &Database, sql: &str) -> i64 {
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(sql)
                .fetch_one(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("count")
        }
        Backend::Postgres => {
            sqlx::query_scalar(sql)
                .fetch_one(db.postgres_pool().expect("postgres"))
                .await
                .expect("count")
        }
    }
}

async fn seed(db: &Database) {
    exec(
        db,
        &format!(
            "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version)
             VALUES ('{ACCOUNT}','active','tagbf@example.invalid','adult',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at,
                                 updated_at, version)
             VALUES ('{PSEUD}','{ACCOUNT}','tagbf','Tag Backfill','listed',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;
    for work in [WORK_A, WORK_B] {
        exec(
            db,
            &format!(
                "INSERT INTO works (id, owner_pseud_id, title, summary, visibility, lifecycle,
                                    created_at, updated_at, version)
                 VALUES ('{work}','{PSEUD}','Work {work}','summary','public','published',
                         '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
            ),
        )
        .await;
    }
    exec(
        db,
        &format!(
            "INSERT INTO library_items (id, account_id, work_id, source_key, source_work_key, title,
                                        author_text, source_url, likes, provenance_json,
                                        created_at, updated_at, version)
             VALUES ('01956c00-0000-4000-8000-000000000011','{ACCOUNT}','{WORK_A}','archive','a1',
                     'A','Someone','https://example.invalid/a',0,
                     '{{\"fandom\":\"Naruto\",\"tags\":[\"Canon\",\"AU\"],\"characters\":[\"Naruto Uzumaki\"]}}',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO library_items (id, account_id, work_id, source_key, source_work_key, title,
                                        author_text, source_url, likes, provenance_json,
                                        created_at, updated_at, version)
             VALUES ('01956c00-0000-4000-8000-000000000012','{ACCOUNT}','{WORK_B}','pawchive','p1',
                     'B','Someone','https://example.invalid/b',0,
                     '{{\"fandom\":\"Avatar: The Last Airbender\",\"tags\":[\"A song of ice and fire\"],\"genre\":[\"Crossover\"]}}',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;
}

#[tokio::test]
async fn dry_run_writes_nothing() {
    let (db, dir) = database("dry").await;
    seed(&db).await;
    let report = backfill_from_provenance(&db, None, 100, true)
        .await
        .expect("dry run");
    assert_eq!(report.scanned, 2, "{report:?}");
    assert_eq!(report.assertions, 7, "{report:?}");
    assert_eq!(report.works_tagged, 2, "{report:?}");
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM work_tags").await,
        0,
        "dry run wrote tags"
    );
    drop(db);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn backfill_writes_source_tags_and_is_idempotent() {
    let (db, dir) = database("real").await;
    seed(&db).await;
    let first = backfill_from_provenance(&db, None, 100, false)
        .await
        .expect("backfill");
    assert_eq!(first.works_tagged, 2, "{first:?}");
    assert_eq!(first.assertions, 7, "{first:?}");
    let work_a_tags: i64 = count(
        &db,
        &format!(
            "SELECT COUNT(*) FROM work_tags WHERE work_id = '{WORK_A}' AND source = 'imported'"
        ),
    )
    .await;
    assert_eq!(work_a_tags, 4, "Naruto/Canon/AU/Naruto Uzumaki");
    let fandom_nodes: i64 = count(&db, "SELECT COUNT(*) FROM canonical_entities WHERE kind='fandom'")
        .await;
    assert_eq!(fandom_nodes, 2, "{fandom_nodes}");

    let second = backfill_from_provenance(&db, None, 100, false)
        .await
        .expect("second pass");
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM work_tags").await,
        first.assertions as i64,
        "second pass piled rows up: {second:?}"
    );
    drop(db);
    let _ = std::fs::remove_dir_all(dir);
}
