//! M63-37 — per-source engagement keeps each archive's own metrics, so a rate is computable.
//!
//! The property is the **denominator's provenance**: `likes_per_view` must divide one archive's endorsements by *that same archive's*
//! hits, and must answer `None` — not `0.0` — when either half is absent or the denominator is zero. A zero rate and an unmeasurable one
//! are different facts; a ranking that confuses them promotes a story nobody measured.
//!
//! Asserts:
//! 1. each metric is stored under its own name and read back;
//! 2. a corrected counter replaces the reading, and `reported_at` travels with it;
//! 3. a rate is computed from one archive's own pair;
//! 4. a missing denominator is `None`, not zero;
//! 5. zero views is `None`, not zero;
//! 6. a negative count is refused at the write;
//! 7. two items from different archives keep their own metrics — no cross-archive division.

use darkhold_db::source_engagement::{likes_per_view, readings, record, Metric};
use darkhold_db::{Backend, Database, DatabaseConfig};
use std::path::PathBuf;

const AUTHOR: &str = "01955c00-0000-4000-8000-0000000000a1";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-eng-{tag}-{}-{:?}",
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
                "lh_test_eng_{}_{}",
                std::process::id(),
                COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            );
            test_support::empty_postgres(&admin_url, &name)
                .await
                .expect("an empty PostgreSQL database")
        }
        _ => {
            let config = DatabaseConfig::new(format!(
                "sqlite://{}/eng.sqlite?mode=rwc",
                dir.display()
            ));
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

/// Insert one imported item and return its id.
async fn item(db: &Database, index: i32, source: &str, source_key: &str) -> String {
    let id = format!("01956000-0000-4000-8000-0000000000{index:02}");
    exec(
        db,
        &format!(
            "INSERT INTO library_items (id, account_id, source_key, source_work_key, title, author_text,
                                        summary, status, source_url, likes, provenance_json,
                                        created_at, updated_at, version)
             VALUES ('{id}','{AUTHOR}','{source}','{source_key}','T','A','','complete',
                     'https://example.invalid/{index}',0,'{{}}',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;
    id
}

#[tokio::test]
async fn a_rate_comes_from_one_archives_own_pair_of_counts() {
    let (db, dir) = database("rate").await;
    exec(
        &db,
        &format!(
            "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version)
             VALUES ('{AUTHOR}','active','e@example.invalid','adult',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;

    let ao3 = item(&db, 1, "ao3", "k1").await;
    let sb = item(&db, 2, "sb", "k2").await;

    // 1. Both metrics stored under their own names.
    record(&db, &ao3, Metric::Hits, 1_000, "2026-01-01T00:00:00Z")
        .await
        .expect("hits");
    record(&db, &ao3, Metric::Kudos, 50, "2026-01-01T00:00:00Z")
        .await
        .expect("kudos");
    let stored = readings(&db, &ao3).await.expect("readings");
    assert_eq!(
        stored.iter().map(|r| r.metric.as_str()).collect::<Vec<_>>(),
        vec!["hits", "kudos"]
    );

    // 3. The rate is 50/1000 — one archive's pair, not a normalised guess.
    let rate = likes_per_view(&db, &ao3, Metric::Kudos, Metric::Hits)
        .await
        .expect("rate");
    assert_eq!(rate, Some(0.05));

    // 2. A corrected counter replaces the reading and carries its own timestamp.
    record(&db, &ao3, Metric::Hits, 2_000, "2026-02-01T00:00:00Z")
        .await
        .expect("corrected");
    let hits = readings(&db, &ao3)
        .await
        .expect("readings")
        .into_iter()
        .find(|r| r.metric == "hits")
        .expect("hits row");
    assert_eq!(hits.value, 2_000);
    assert_eq!(hits.reported_at, "2026-02-01T00:00:00Z");
    assert_eq!(
        likes_per_view(&db, &ao3, Metric::Kudos, Metric::Hits)
            .await
            .expect("rate"),
        Some(0.025)
    );

    // 4. A missing half is `None`, not zero.
    record(&db, &sb, Metric::Likes, 30, "2026-01-01T00:00:00Z")
        .await
        .expect("likes");
    assert_eq!(
        likes_per_view(&db, &sb, Metric::Likes, Metric::Views)
            .await
            .expect("rate"),
        None,
        "the board publishes likes and not views; that is not a rate of zero"
    );

    // 5. Zero views is undefined, not zero.
    record(&db, &sb, Metric::Views, 0, "2026-01-01T00:00:00Z")
        .await
        .expect("views");
    assert_eq!(
        likes_per_view(&db, &sb, Metric::Likes, Metric::Views)
            .await
            .expect("rate"),
        None
    );

    // 7. The two items do not borrow each other's counters.
    assert_eq!(
        likes_per_view(&db, &ao3, Metric::Likes, Metric::Hits)
            .await
            .expect("rate"),
        None,
        "AO3 reports kudos, not likes: a rate must not fall back to another name"
    );

    // 6. Negative counts are refused.
    assert!(
        record(&db, &ao3, Metric::Hits, -1, "2026-02-01T00:00:00Z")
            .await
            .is_err(),
        "a count below zero is a parse bug, not a measurement"
    );

    let _ = std::fs::remove_dir_all(dir);
}
