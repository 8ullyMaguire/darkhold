//! M70-07 — the queue: idempotent, newest-first, and unreachable from another account.
//!
//! The privacy assertion is the point of the row: this table is read only through a function that takes an account,
//! so a test that queues for one account and reads for another is the one that proves there is no unscoped path.

use darkhold_db::Database;

fn scratch_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-mfl-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

async fn database(tag: &str) -> (Database, std::path::PathBuf) {
    let dir = scratch_dir(tag);
    let db = match std::env::var("DARKHOLD_TEST_PG_URL") {
        Ok(admin_url) if !admin_url.is_empty() => {
            static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let name = format!(
                "lh_test_mfl_{}_{}",
                std::process::id(),
                COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            );
            test_support::empty_postgres(&admin_url, &name)
                .await
                .expect("an empty PostgreSQL database")
        }
        _ => {
            let config = darkhold_db::DatabaseConfig::new(format!(
                "sqlite://{}/darkhold.sqlite?mode=rwc",
                dir.display()
            ));
            Database::connect(&config).await.expect("connect")
        }
    };
    db.migrate().await.expect("migrate");
    (db, dir)
}

async fn scalar_count(db: &Database, statement: &str) -> i64 {
    match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar(statement)
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("count"),
        darkhold_db::Backend::Postgres => sqlx::query_scalar(statement)
            .fetch_one(db.postgres_pool().expect("postgres"))
            .await
            .expect("count"),
    }
}

async fn exec(db: &Database, statement: &str) {
    let result = match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query(statement)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await
            .map(|_| ()),
        darkhold_db::Backend::Postgres => sqlx::query(statement)
            .execute(db.postgres_pool().expect("postgres"))
            .await
            .map(|_| ()),
    };
    result.unwrap_or_else(|error| panic!("{statement}\n{error}"));
}

/// Two accounts, each with one published work. Ids are UUID-shaped: PostgreSQL stores them as `uuid` and an id like
/// `acc-1` is a cast error on that arm only.
const A1: &str = "01955000-0000-4000-8000-0000000000a1";
const A2: &str = "01955000-0000-4000-8000-0000000000a2";
const P1: &str = "01955000-0000-4000-8000-0000000000b1";
const P2: &str = "01955000-0000-4000-8000-0000000000b2";
const W1: &str = "01957000-0000-4000-8000-0000000000c1";
const W2: &str = "01957000-0000-4000-8000-0000000000c2";

async fn seed(db: &Database) {
    for (account, pseud, handle) in [(A1, P1, "one"), (A2, P2, "two")] {
        exec(
            db,
            &format!(
                "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version) \
                 VALUES ('{account}','active','{handle}@mfl.invalid','adult', \
                         '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
            ),
        )
        .await;
        exec(
            db,
            &format!(
                "INSERT INTO pseuds (id, account_id, handle, display_name, bio, discoverability, created_at, \
                 updated_at, version) VALUES ('{pseud}','{account}','{handle}','{handle}',NULL,'listed', \
                 '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
            ),
        )
        .await;
    }
    for (work, pseud, title) in [(W1, P1, "First"), (W2, P2, "Second")] {
        exec(
            db,
            &format!(
                "INSERT INTO works (id, owner_pseud_id, title, summary, language, rating, visibility, lifecycle, \
                 completion, created_at, updated_at, version) VALUES ('{work}','{pseud}','{title}','','en', \
                 'general','public','published','complete','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
            ),
        )
        .await;
    }
}

#[tokio::test]
async fn queueing_is_idempotent_and_the_queue_is_per_account_newest_first() {
    let (db, dir) = database("queue").await;
    seed(&db).await;

    assert!(
        darkhold_db::marked_for_later::mark(&db, A1, W1)
            .await
            .expect("mark"),
        "the first queueing adds the entry"
    );
    assert!(
        !darkhold_db::marked_for_later::mark(&db, A1, W1)
            .await
            .expect("mark again"),
        "queueing twice is not two entries, and not an error"
    );
    assert!(darkhold_db::marked_for_later::is_marked(&db, A1, W1)
        .await
        .expect("is_marked"));

    // A second work, queued later, sorts first.
    assert!(darkhold_db::marked_for_later::mark(&db, A1, W2)
        .await
        .expect("mark second"));
    let queue = darkhold_db::marked_for_later::queue(&db, A1, 10)
        .await
        .expect("queue");
    assert_eq!(queue.len(), 2);
    assert_eq!(
        queue[0].work_id, W2,
        "newest first: the queue is 'what next', not a history"
    );
    assert_eq!(queue[1].work_id, W1);

    // **Another account sees nothing**, and gets no count either: the query is scoped, not filtered afterwards.
    assert!(darkhold_db::marked_for_later::queue(&db, A2, 10)
        .await
        .expect("other queue")
        .is_empty());
    assert!(!darkhold_db::marked_for_later::is_marked(&db, A2, W1)
        .await
        .expect("other is_marked"));

    // Dropping is idempotent the same way round.
    assert!(darkhold_db::marked_for_later::unmark(&db, A1, W1)
        .await
        .expect("unmark"));
    assert!(!darkhold_db::marked_for_later::unmark(&db, A1, W1)
        .await
        .expect("unmark twice"));
    assert_eq!(
        darkhold_db::marked_for_later::queue(&db, A1, 10)
            .await
            .expect("queue")
            .len(),
        1
    );

    // Deleting the account takes its queue with it, and leaves the other's alone.
    exec(&db, &format!("DELETE FROM accounts WHERE id = '{A1}'")).await;
    assert_eq!(
        scalar_count(&db, "SELECT count(*) FROM marked_for_later").await,
        0,
        "the cascade reaches this table"
    );

    let _ = std::fs::remove_dir_all(dir);
}
