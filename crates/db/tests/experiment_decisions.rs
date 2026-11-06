//! M64-04 — a promotion is one row, and it carries the argument for itself.
//!
//! The decision function is tested in the domain. What is tested here is the half that survives a restart: that a winning
//! challenger produces exactly one `promote` row whose algorithms describe the swap, that an empty week produces no row at
//! all, and that a demotion is written with its guardrail rather than as a shrug.

use darkhold_db::Database;

fn scratch_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-decisions-{tag}-{}-{:?}",
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
                "lh_test_dec_{}_{}",
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

/// `days` snapshots for one arm with the given per-day impressions and conversion rate.
async fn snapshots(db: &Database, arm: i64, days: u32, impressions: i64, conversions: i64) {
    for day in 1..=days {
        // A UUID-shaped id in hex digits only, because PostgreSQL stores ids as `uuid` and a non-hex id is a cast error on
        // that arm but a fine string on the sqlite one.
        let id = format!("0195500{day}-0000-4000-8000-{arm}00000000000");
        let label = format!("2026-11-{day:02}");
        exec(
            db,
            &format!(
                "INSERT INTO experiment_snapshots \
                 (id, surface_id, arm, day, impressions, readers, guest_readers, events, computed_at) \
                 VALUES ('{id}', 'discover.feed', {arm}, '{label}', {impressions}, 10, 0, {conversions}, \
                         '2026-11-01T00:00:00Z')"
            ),
        )
        .await;
    }
}

#[tokio::test]
async fn a_winning_challenger_is_recorded_once_with_the_swap_it_makes() {
    let (db, dir) = database("promote").await;
    // Eight days, 1000 impressions and 100 vs 120 conversions a day: 8000 impressions and 800 vs 960 clicks.
    snapshots(&db, 0, 8, 1000, 100).await;
    snapshots(&db, 1, 8, 1000, 120).await;

    let before = vec!["popular".to_owned(), "tag_jaccard".to_owned()];
    let after = vec!["popular".to_owned()];
    let decision = darkhold_db::experiment_decisions::evaluate_surface(
        &db,
        "discover.feed",
        "2026-11-08",
        0,
        1,
        (&before, &after),
        Vec::new(),
    )
    .await
    .expect("evaluate");
    assert!(
        matches!(
            decision,
            darkhold_domain::experiment_promotion::Decision::Promote { .. }
        ),
        "800 against 960 clicks over eight days is a win: {decision:?}"
    );

    let rows = darkhold_db::experiment_decisions::decisions_for(&db, "discover.feed")
        .await
        .expect("read back");
    assert_eq!(rows.len(), 1, "one verdict, one row: {rows:?}");
    assert_eq!(rows[0].decision, "promote");
    assert_eq!(rows[0].incumbent_arm, 0);
    assert_eq!(rows[0].challenger_arm, 1);
    assert!(
        rows[0].lift > 0.0 && rows[0].evidence >= 20.0,
        "the row carries the numbers it was decided on: {rows:?}"
    );
    assert_eq!(rows[0].algorithms_before, before);
    assert_eq!(
        rows[0].algorithms_after, after,
        "the swap travels with the decision, or a rollback has to guess"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_week_of_no_traffic_records_nothing() {
    let (db, dir) = database("nothing").await;
    // Seven days observed, no impressions: the sample floor is what stops this being a verdict.
    snapshots(&db, 0, 7, 0, 0).await;
    snapshots(&db, 1, 7, 0, 0).await;

    let decision = darkhold_db::experiment_decisions::evaluate_surface(
        &db,
        "discover.feed",
        "2026-11-07",
        0,
        1,
        (&["popular".to_owned()], &["popular".to_owned()]),
        Vec::new(),
    )
    .await
    .expect("evaluate");
    assert!(
        matches!(
            decision,
            darkhold_domain::experiment_promotion::Decision::InsufficientSample { .. }
        ),
        "{decision:?}"
    );
    assert!(
        darkhold_db::experiment_decisions::decisions_for(&db, "discover.feed")
            .await
            .expect("read back")
            .is_empty(),
        "a non-verdict is returned to the caller and not stored: a month of 'not yet' would bury the rows that matter"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn two_breached_snapshots_are_recorded_as_a_demotion() {
    use darkhold_domain::experiment_promotion::GuardrailReading;

    let (db, dir) = database("demote").await;
    snapshots(&db, 0, 8, 1000, 100).await;
    snapshots(&db, 1, 8, 1000, 120).await;

    let breach = || {
        vec![GuardrailReading {
            name: "latency_p95".to_owned(),
            in_bounds: false,
            value: 900.0,
        }]
    };
    let decision = darkhold_db::experiment_decisions::evaluate_surface(
        &db,
        "discover.feed",
        "2026-11-08",
        0,
        1,
        (
            &["popular".to_owned(), "tag_jaccard".to_owned()],
            &["popular".to_owned()],
        ),
        vec![breach(), breach()],
    )
    .await
    .expect("evaluate");
    assert!(
        matches!(
            decision,
            darkhold_domain::experiment_promotion::Decision::Demote { .. }
        ),
        "a winning challenger still loses to a breached guardrail: {decision:?}"
    );

    let rows = darkhold_db::experiment_decisions::decisions_for(&db, "discover.feed")
        .await
        .expect("read back");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].decision, "demote");
    assert!(
        darkhold_db::experiment_decisions::override_for(&db, "discover.feed")
            .await
            .expect("read the override")
            .is_none(),
        "a demoted challenger must not keep the surface"
    );

    let _ = std::fs::remove_dir_all(dir);
}
