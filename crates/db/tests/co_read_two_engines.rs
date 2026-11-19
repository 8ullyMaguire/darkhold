//! M64-12 — co-read transitions on both engines.

use darkhold_db::co_read::{also_enjoyed, Options};
use darkhold_db::{Backend, Database};
use test_support::TestDb;

const A: &str = "01955700-0000-4000-8000-000000000001";
const B: &str = "01955700-0000-4000-8000-000000000002";
const C: &str = "01955700-0000-4000-8000-000000000003";
const ACCOUNT: &str = "01955700-0000-4000-8000-000000000011";
const PSEUD: &str = "01955700-0000-4000-8000-000000000012";

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

async fn fixture(tag: &str) -> TestDb {
    let dir = std::env::temp_dir().join(format!("darkhold-co-read-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let tdb = TestDb::connect_with_dir(tag, &dir).await;
    let db = tdb.db();
    exec(db, &format!("INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version) VALUES ('{ACCOUNT}','active','reader@test.dev','adult','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)")).await;
    exec(db, &format!("INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at, updated_at, version) VALUES ('{PSEUD}','{ACCOUNT}','reader','Reader','listed','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)")).await;
    for (id, title) in [(A, "A"), (B, "B"), (C, "C")] {
        exec(db, &format!("INSERT INTO works (id, owner_pseud_id, title, visibility, lifecycle, created_at, updated_at) VALUES ('{id}','{PSEUD}','{title}','public','published','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')")).await;
    }
    tdb
}

#[tokio::test]
async fn co_read_counts_only_long_sessions_in_the_window() {
    let tdb = fixture("co-read").await;
    let db = tdb.db();
    // A -> B qualifies: both legs are long and the target starts after the source ends.
    exec(db, &format!("INSERT INTO reading_sessions (id, account_id, work_id, seconds, started_at, ended_at) VALUES ('01955700-0000-4000-8000-000000000101','{ACCOUNT}','{A}',60,'2026-11-10T10:00:00Z','2026-11-10T10:01:00Z')")).await;
    exec(db, &format!("INSERT INTO reading_sessions (id, account_id, work_id, seconds, started_at, ended_at) VALUES ('01955700-0000-4000-8000-000000000102','{ACCOUNT}','{B}',90,'2026-11-10T10:05:00Z','2026-11-10T10:06:30Z')")).await;
    // C is a single click and must not become an edge.
    exec(db, &format!("INSERT INTO reading_sessions (id, account_id, work_id, seconds, started_at, ended_at) VALUES ('01955700-0000-4000-8000-000000000103','{ACCOUNT}','{C}',3,'2026-11-10T10:07:00Z','2026-11-10T10:07:03Z')")).await;

    let found = also_enjoyed(
        db,
        A,
        &Options {
            since: "2026-11-01T00:00:00Z".to_owned(),
            until: "2026-11-20T00:00:00Z".to_owned(),
            min_readers: 1,
            limit: 10,
        },
    )
    .await
    .expect("co-read");
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, B);
    assert_eq!(found[0].1, 1);
}
