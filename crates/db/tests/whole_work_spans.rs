//! M70-02 — chapter spans on both engines.
//!
//! The app-level test proves the route; this one proves the query, because the two arms differ in places a green
//! SQLite run cannot see: `word_count` is INT4 on PostgreSQL and comes back cast to `bigint`, and the chapter id is
//! `uuid` there and `TEXT` here. Four PostgreSQL-only defects shipped in this repository precisely because the
//! second arm had never run.

use darkhold_db::Database;

fn scratch_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-spans-{tag}-{}-{:?}",
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
                "lh_test_spans_{}_{}",
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

const WORK: &str = "01957000-0000-4000-8000-0000000000c1";
const PSEUD: &str = "01955000-0000-4000-8000-0000000000b1";

async fn seed(db: &Database) {
    exec(
        db,
        "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version) \
         VALUES ('01955000-0000-4000-8000-0000000000a1','active','reader@spans.invalid','adult', \
                 '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)",
    )
    .await;
    exec(
        db,
        "INSERT INTO pseuds (id, account_id, handle, display_name, bio, discoverability, created_at, \
         updated_at, version) \
         VALUES ('01955000-0000-4000-8000-0000000000b1','01955000-0000-4000-8000-0000000000a1', \
                 'hollowgast','hollowgast',NULL,'listed','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)",
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO works (id, owner_pseud_id, title, summary, language, rating, visibility, lifecycle, \
             completion, created_at, updated_at, version) \
             VALUES ('{WORK}','{PSEUD}','The Long Way Round','','en','general','public','published','complete', \
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;
    // Order keys 10, 30, 20 — inserted out of order on purpose: the query's ORDER BY is what the reader relies on,
    // and an implementation that returned insertion order would pass a test seeded in reading order.
    for (id, key, title) in [
        ("01956000-0000-4000-8000-000000000c01", 10, "One"),
        ("01956000-0000-4000-8000-000000000c03", 30, "Three"),
        ("01956000-0000-4000-8000-000000000c02", 20, "Two"),
    ] {
        exec(
            db,
            &format!(
                "INSERT INTO units (id, work_id, order_key, title, current_revision_id, created_at, updated_at, \
                 version) VALUES ('{id}','{WORK}',{key},'{title}',NULL, \
                 '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
            ),
        )
        .await;
    }
    // Chapter one: 100 words. Chapter three: 40. Chapter two stays a draft.
    for (unit, revision, words) in [
        (
            "01956000-0000-4000-8000-000000000c01",
            "01956000-0000-4000-8000-0000000000d1",
            100,
        ),
        (
            "01956000-0000-4000-8000-000000000c03",
            "01956000-0000-4000-8000-0000000000d3",
            40,
        ),
    ] {
        exec(
            db,
            &format!(
                "INSERT INTO revisions (id, unit_id, revision_number, document_json, sanitized_html, plain_text, \
                 word_count, created_by_pseud_id, created_at) \
                 VALUES ('{revision}','{unit}',1,'{{}}','<p>x</p>','x',{words},'{PSEUD}','2026-01-01T00:00:00Z')"
            ),
        )
        .await;
        exec(
            db,
            &format!("UPDATE units SET current_revision_id = '{revision}' WHERE id = '{unit}'"),
        )
        .await;
    }
}

#[tokio::test]
async fn spans_are_ordered_by_the_author_and_cumulative() {
    let (db, dir) = database("ordered").await;
    seed(&db).await;

    let spans = darkhold_db::whole_work::spans(&db, WORK)
        .await
        .expect("spans");
    let order: Vec<&str> = spans.iter().map(|s| s.title.as_str()).collect();
    assert_eq!(
        order,
        ["One", "Two", "Three"],
        "insertion order would be One, Three, Two"
    );
    let starts: Vec<u64> = spans.iter().map(|s| s.start).collect();
    assert_eq!(
        starts,
        [0, 100, 100],
        "the draft chapter adds nothing to the running total"
    );
    let words: Vec<u64> = spans.iter().map(|s| s.words).collect();
    assert_eq!(words, [100, 0, 40]);
    assert_eq!(
        darkhold_db::whole_work::total_words(&db, WORK)
            .await
            .expect("total"),
        140
    );

    let _ = std::fs::remove_dir_all(dir);
}
