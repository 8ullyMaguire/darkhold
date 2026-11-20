//! TA2-3 — Pawchive chapter submissions share one canonical library work.

use darkhold_db::imports::{
    find_library_item_by_source_key, group_pawchive_library_items, pawchive_work_key,
};
use darkhold_db::{Backend, Database, DatabaseConfig};
use std::path::PathBuf;

const ACCOUNT: &str = "01956c00-0000-4000-8000-000000000001";
const ITEM: &str = "01956c00-0000-4000-8000-000000000011";
const ITEM_2: &str = "01956c00-0000-4000-8000-000000000012";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-pawchive-group-{tag}-{}-{:?}",
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
                "lh_test_pawchgrp_{}_{}",
                std::process::id(),
                COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            );
            test_support::empty_postgres(&admin_url, &name)
                .await
                .expect("an empty PostgreSQL database")
        }
        _ => {
            let config =
                DatabaseConfig::new(format!("sqlite://{}/pawchgrp.sqlite?mode=rwc", dir.display()));
            Database::connect(&config).await.expect("connect")
        }
    };
    db.migrate().await.expect("migrate");
    (db, dir)
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

#[test]
fn chapter_titles_normalise_to_one_key() {
    assert_eq!(
        pawchive_work_key("Dark MC Ch. 1", "Vandal"),
        pawchive_work_key("dark  mc ch  1", "VANDAL")
    );
    assert_ne!(
        pawchive_work_key("Dark MC Ch. 1", "Vandal"),
        pawchive_work_key("Dark MC Ch. 2", "Other Author")
    );
}

#[tokio::test]
async fn canonical_key_finds_the_one_library_item() {
    let (db, _dir) = database("find").await;
    exec(
        &db,
        &format!(
            "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version)
             VALUES ('{ACCOUNT}','active','pawchgrp@example.invalid','adult',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;
    let key = pawchive_work_key("Dark MC Ch. 1", "Vandal");
    exec(
        &db,
        &format!(
            "INSERT INTO library_items (id, account_id, source_key, source_work_key, title,
                                        author_text, source_url, provenance_json,
                                        created_at, updated_at, version)
             VALUES ('{ITEM}','{ACCOUNT}','pawchive','{key}','Dark MC Ch. 1','Vandal',
                     'https://pawchive.pw/p/1',
                     '{{\"source_urls\":[\"https://pawchive.pw/p/1\"]}}',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;
    let found = find_library_item_by_source_key(&db, ACCOUNT, "pawchive", &key)
        .await
        .expect("find")
        .expect("one item");
    assert_eq!(found.id, ITEM);
    assert!(found.provenance_json.contains("source_urls"));
    assert!(find_library_item_by_source_key(&db, ACCOUNT, "pawchive", "missing")
        .await
        .expect("missing")
        .is_none());
}

#[tokio::test]
async fn repair_merges_duplicate_chapter_items() {
    let (db, _dir) = database("repair").await;
    exec(
        &db,
        &format!(
            "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version)
             VALUES ('{ACCOUNT}','active','pawgrp@example.invalid','adult',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;
    let key = pawchive_work_key("Dark MC Ch. 1", "Vandal");
    for (id, source_key, title, url, created) in [
        (ITEM, "p1", "Dark MC Ch. 1", "https://pawchive.pw/p/1", "2026-01-01T00:00:00Z"),
        (ITEM_2, "p2", "dark  mc ch  1", "https://pawchive.pw/p/2", "2026-01-02T00:00:00Z"),
    ] {
        exec(
            &db,
            &format!(
                "INSERT INTO library_items (id, account_id, source_key, source_work_key, title,
                                            author_text, source_url, provenance_json,
                                            created_at, updated_at, version)
                 VALUES ('{id}','{ACCOUNT}','pawchive','{source_key}','{title}','Vandal','{url}',
                         '{{\"source_urls\":[\"{url}\"]}}',
                         '{created}','{created}',1)"
            ),
        )
        .await;
    }
    let dry = group_pawchive_library_items(&db, ACCOUNT, true)
        .await
        .expect("dry run");
    assert_eq!(dry.groups, 1, "{dry:?}");
    assert_eq!(dry.merged, 1, "{dry:?}");
    assert_eq!(
        count(&db, "SELECT count(*) FROM library_items WHERE source_key = 'pawchive'").await,
        2
    );
    let applied = group_pawchive_library_items(&db, ACCOUNT, false)
        .await
        .expect("apply");
    assert_eq!(applied.merged, 1, "{applied:?}");
    assert_eq!(
        count(&db, "SELECT count(*) FROM library_items WHERE source_key = 'pawchive'").await,
        1
    );
    let keeper = find_library_item_by_source_key(&db, ACCOUNT, "pawchive", &key)
        .await
        .expect("find")
        .expect("keeper");
    assert_eq!(keeper.id, ITEM);
    assert!(keeper.provenance_json.contains("https://pawchive.pw/p/1"));
    assert!(keeper.provenance_json.contains("https://pawchive.pw/p/2"));
}
