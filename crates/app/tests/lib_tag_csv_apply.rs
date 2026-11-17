//! M63-11: the CSV loader applies through the product write path on both engines.

use darkhold_app::lib_tag::{apply, parse_tag_csv, rollback_sql};
use darkhold_db::Database;

fn scratch(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("darkhold-lib-tag-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

async fn exec(db: &Database, sql: &str) {
    match db.backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(sql)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("statement");
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(sql)
                .execute(db.postgres_pool().expect("postgres"))
                .await
                .expect("statement");
        }
    }
}

async fn count_tags(db: &Database, work: &str, tag: &str) -> i64 {
    let sql = match db.backend() {
        darkhold_db::Backend::Sqlite => {
            "SELECT COUNT(*) FROM work_tags WHERE work_id = ? AND node_id = ?"
        }
        darkhold_db::Backend::Postgres => {
            "SELECT COUNT(*)::bigint FROM work_tags WHERE work_id::text = $1 AND node_id = $2"
        }
    };
    match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar::<_, i64>(sql)
            .bind(work)
            .bind(tag)
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("count"),
        darkhold_db::Backend::Postgres => sqlx::query_scalar::<_, i64>(sql)
            .bind(work)
            .bind(tag)
            .fetch_one(db.postgres_pool().expect("postgres"))
            .await
            .expect("count"),
    }
}

#[tokio::test]
async fn a_valid_batch_is_applied_and_each_row_has_a_targeted_rollback() {
    let dir = scratch("apply");
    let tdb = test_support::TestDb::connect_with_dir("lib-tag-apply", &dir).await;
    let db = tdb.db();
    let work = "11111111-1111-4111-8111-111111111111";
    let account = "22222222-2222-4222-8222-222222222222";
    let tag = "33333333-3333-4333-8333-333333333333";
    let now = "2026-01-01T00:00:00Z";
    exec(
        db,
        &format!("INSERT INTO accounts (id, email, created_at, updated_at) VALUES ('{account}', 'libtag@x.test', '{now}', '{now}')"),
    )
    .await;
    exec(
        db,
        &format!("INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at) VALUES ('{account}', '{account}', 'libtag', 'Lib', '{now}', '{now}')"),
    )
    .await;
    exec(
        db,
        &format!("INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at) VALUES ('{work}', '{account}', 'Tagged', '{now}', '{now}')"),
    )
    .await;
    exec(
        db,
        &format!("INSERT INTO canonical_entities (id, kind, canonical, norm, created_at, review_status, signal_count, node_state, node_count) VALUES ('{tag}', 'tag', 'dragons', 'dragons', '{now}', 'curated', 0, 'browsable', 10)"),
    )
    .await;

    let csv = format!("work_id,tag\n{work},dragons\n");
    let parsed = parse_tag_csv(&csv);
    let applied = apply(db, &parsed).await.expect("the batch");
    assert_eq!(applied.len(), 1);
    assert_eq!(count_tags(db, work, "dragons").await, 1);

    let sql = rollback_sql(&applied[0]);
    assert!(sql.contains(&format!("work_id='{work}'")));
    assert!(sql.contains("node_id='dragons'"));
    assert!(sql.contains("AND added_at="));
}
