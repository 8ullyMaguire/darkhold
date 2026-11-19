//! M64-13 — reader-overlap counts on both engines.

use darkhold_db::engagement_overlap::{reader_overlap, Options};
use darkhold_db::{Backend, Database};
use test_support::TestDb;

const A: &str = "01955700-0000-4000-8000-000000000001";
const B: &str = "01955700-0000-4000-8000-000000000002";

async fn exec(db: &Database, sql: &str) {
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(sql)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("fixture");
        }
        Backend::Postgres => {
            sqlx::query(sql)
                .execute(db.postgres_pool().expect("postgres"))
                .await
                .expect("fixture");
        }
    }
}

#[tokio::test]
async fn reader_overlap_counts_distinct_readers_only() {
    let dir = std::env::temp_dir().join(format!("darkhold-overlap-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let tdb = TestDb::connect_with_dir("overlap", &dir).await;
    let db = tdb.db();
    let owner = "01955700-0000-4000-8000-0000000000ff";
    let owner_pseud = "01955700-0000-4000-8000-0000000000fe";
    exec(db, &format!("INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version) VALUES ('{owner}','active','owner@test.dev','adult','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)")).await;
    exec(db, &format!("INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at, updated_at, version) VALUES ('{owner_pseud}','{owner}','owner','Owner','listed','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)")).await;
    for (id, title) in [(A, "A"), (B, "B")] {
        exec(db, &format!("INSERT INTO works (id, owner_pseud_id, title, visibility, lifecycle, created_at, updated_at) VALUES ('{id}','{owner_pseud}','{title}','public','published','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')")).await;
    }
    for index in 0..10 {
        let account = format!("01955700-0000-4000-8000-0000000001{:02}", index);
        let pseud = format!("01955700-0000-4000-8000-0000000002{:02}", index);
        exec(db, &format!("INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version) VALUES ('{account}','active','r{index}@test.dev','adult','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)")).await;
        exec(db, &format!("INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at, updated_at, version) VALUES ('{pseud}','{account}','r{index}','R{index}','listed','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)")).await;
        let session_a = format!("01955700-0000-4000-8000-0000000003{:02}", index);
        exec(db, &format!("INSERT INTO reading_sessions (id, account_id, work_id, seconds, started_at, ended_at) VALUES ('{session_a}','{account}','{A}',60,'2026-11-10T10:00:00Z','2026-11-10T10:01:00Z')")).await;
        if index < 9 {
            let session_b = format!("01955700-0000-4000-8000-0000000004{:02}", index);
            exec(db, &format!("INSERT INTO reading_sessions (id, account_id, work_id, seconds, started_at, ended_at) VALUES ('{session_b}','{account}','{B}',90,'2026-11-10T10:05:00Z','2026-11-10T10:06:30Z')")).await;
        }
    }
    let options = Options {
        since: "2026-11-01T00:00:00Z".to_owned(),
        until: "2026-11-20T00:00:00Z".to_owned(),
    };
    let count = reader_overlap(db, A, B, &options).await.expect("overlap");
    assert_eq!(count, 9, "nine readers overlap on both works");
}
