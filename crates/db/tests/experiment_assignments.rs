//! M64-02 — the arm is assigned once and then read.
//!
//! The requirement is "a sticky bucket assignment per pseud", and stickiness is the only part a unit test cannot reach:
//! the value has to survive a second read *and* a change to the arm count, because an operator raising the number of arms
//! is exactly the moment a recomputed bucket would silently move readers and destroy the comparison.
//!
//! What these tests do not claim: that the split is *fair*. Fairness is a property of the hash over a population, not of
//! one reader, and a test that asserted "the first 1000 pseuds split evenly" would be measuring FNV-1a rather than the
//! assignment. The spread test asserts only that the split is not constant, which is the failure that would make an
//! experiment meaningless.

use darkhold_db::Database;

const ACCOUNT: &str = "01955000-0000-4000-8000-0000000000e1";
const PSEUD: &str = "01955000-0000-4000-8000-0000000000e2";

fn scratch_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-experiments-{tag}-{}-{:?}",
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
                "lh_test_assign_{}_{}",
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
    match db.backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(statement)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await
                .unwrap_or_else(|error| panic!("{statement}\n{error}"));
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(statement)
                .execute(db.postgres_pool().expect("postgres"))
                .await
                .unwrap_or_else(|error| panic!("{statement}\n{error}"));
        }
    }
}

async fn seed(db: &Database) {
    exec(
        db,
        &format!(
            "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version) \
             VALUES ('{ACCOUNT}', 'active', 'assign@mfl.invalid', 'adult', \
                     '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1)"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, bio, discoverability, created_at, \
             updated_at, version) VALUES ('{PSEUD}', '{ACCOUNT}', 'assign', 'Assign', NULL, 'listed', \
             '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1)"
        ),
    )
    .await;
}

async fn assigned(db: &Database) -> i64 {
    let sql = match db.backend() {
        darkhold_db::Backend::Sqlite => {
            "SELECT count(*) FROM experiment_assignments WHERE pseud_id = ?1".to_string()
        }
        darkhold_db::Backend::Postgres => {
            "SELECT count(*) FROM experiment_assignments WHERE pseud_id = $1::uuid".to_string()
        }
    };
    let count: i64 = match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar(&sql)
            .bind(PSEUD)
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("count"),
        darkhold_db::Backend::Postgres => sqlx::query_scalar(&sql)
            .bind(PSEUD)
            .fetch_one(db.postgres_pool().expect("postgres"))
            .await
            .expect("count"),
    };
    count
}

#[tokio::test]
async fn the_arm_is_written_once_and_survives_a_new_arm_count() {
    let (db, dir) = database("sticky").await;
    seed(&db).await;

    let first = darkhold_db::experiments::bucket_for(&db, PSEUD, "discover.feed", 2)
        .await
        .expect("first assignment");
    let again = darkhold_db::experiments::bucket_for(&db, PSEUD, "discover.feed", 2)
        .await
        .expect("second read");
    assert_eq!(
        first, again,
        "the same reader, the same surface, the same arm"
    );

    // The operator doubles the arm count. The reader must not move: 1 in 2 becomes 1 in 4 by adding arms for *other*
    // readers, and a reader who changed arm here would make every earlier impression unusable.
    let widened = darkhold_db::experiments::bucket_for(&db, PSEUD, "discover.feed", 4)
        .await
        .expect("after the arm count changed");
    assert_eq!(
        first, widened,
        "a stored arm is the assignment; a bigger arm count does not reassign anyone"
    );
    assert_eq!(assigned(&db).await, 1, "one row per (pseud, surface)");

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_surface_is_assigned_independently_of_another() {
    let (db, dir) = database("surfaces").await;
    seed(&db).await;

    darkhold_db::experiments::bucket_for(&db, PSEUD, "discover.feed", 2)
        .await
        .expect("feed assignment");
    darkhold_db::experiments::bucket_for(&db, PSEUD, "home.trending", 2)
        .await
        .expect("trending assignment");
    assert_eq!(
        assigned(&db).await,
        2,
        "surface_id is part of the key: one experiment must not set another's arm"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn the_split_is_not_constant() {
    let (db, dir) = database("spread").await;
    seed(&db).await;

    // Fresh pseud ids, because the hash is over the id itself. Twenty is enough to see both arms without pretending to
    // measure fairness (see the module doc).
    let mut arms = std::collections::BTreeSet::new();
    for n in 0..20 {
        let pseud = format!("01955000-0000-4000-8000-{:012x}", n);
        let account = format!("01955000-0000-4000-8001-{:012x}", n);
        // The assignment table has a real foreign key: a bucket for a pseud that does not exist would be an experiment
        // against nobody, so the fixture has to create readers rather than invent ids.
        exec(
            &db,
            &format!(
                "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version) \
                 VALUES ('{account}', 'active', 'spread{n}@mfl.invalid', 'adult', \
                         '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1)"
            ),
        )
        .await;
        exec(
            &db,
            &format!(
                "INSERT INTO pseuds (id, account_id, handle, display_name, bio, discoverability, created_at, \
                 updated_at, version) VALUES ('{pseud}', '{account}', 'spread{n}', 'Spread {n}', NULL, 'listed', \
                 '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1)"
            ),
        )
        .await;
        arms.insert(
            darkhold_db::experiments::bucket_for(&db, &pseud, "discover.feed", 2)
                .await
                .expect("assignment"),
        );
    }
    assert!(
        arms.len() > 1,
        "twenty readers landed in one arm, so the split assigns nothing: {arms:?}"
    );
    for arm in &arms {
        assert!((0..2).contains(arm), "an arm outside the arm count: {arm}");
    }

    let _ = std::fs::remove_dir_all(dir);
}
