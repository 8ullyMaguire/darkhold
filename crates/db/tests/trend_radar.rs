//! M45-32 — the trend radar's store: two windows split in one query, on both engines.
//!
//! # What this asserts, and why each half is needed
//!
//! * **The split is by arrival, not by publication.** `work_tags.added_at` is when this instance learned
//!   the tag; a work published last year and imported yesterday is an arrival today.
//! * **A row lands in exactly one window.** `added_at >= recent_since` is recent, `< recent_since` is
//!   baseline, and a work added after the recent bound counts once — the `CASE WHEN` pair is the whole
//!   contract, and a `<=`/`<` mistake moves works between windows without failing anything that only
//!   checks totals.
//! * **The floor is applied in SQL, not in Rust.** A fandom below `min_recent` is not returned, so a
//!   caller that ignores the rule cannot see a fandom the rule refuses to judge.
//! * **Both engines.** The SQLite and PostgreSQL statements differ in placeholders and casts; the arm
//!   that has never run is the one that breaks (see the four PostgreSQL-only stores in `docs/handoff.md`).

use darkhold_db::Database;

fn scratch_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-trend-{tag}-{}-{:?}",
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
                "lh_test_trend_{}_{}",
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

/// The baseline opens at the start of January; the recent window opens on 1 March.
const BASELINE_SINCE: &str = "2026-01-01T00:00:00Z";
const RECENT_SINCE: &str = "2026-03-01T00:00:00Z";

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

/// Two fandoms at fixed instants: the baseline window is January, the recent window is March, and February
/// is empty so a boundary mistake cannot be excused by adjacency.
///
/// * `emerging-fandom`: 2 arrivals in January, 3 in March — above the floor.
/// * `quiet-fandom`: 5 in January, 1 in March — real, and below the floor, so the store must not return it.
async fn fixture(db: &Database) {
    let owner = uuid::Uuid::new_v4().to_string();
    exec(
        db,
        &format!(
            "INSERT INTO accounts (id, email, created_at, updated_at) \
             VALUES ('{owner}', 'trend-{owner}@x.test', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at) \
             VALUES ('{owner}', '{owner}', 'tr{owner}', 'Trend', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        ),
    )
    .await;

    let emerging = uuid::Uuid::new_v4().to_string();
    let quiet = uuid::Uuid::new_v4().to_string();
    for (id, canonical) in [(&emerging, "emerging-fandom"), (&quiet, "quiet-fandom")] {
        exec(
            db,
            &format!(
                "INSERT INTO canonical_entities \
                 (id, kind, canonical, norm, created_at, review_status, signal_count, node_state, node_count) \
                 VALUES ('{id}', 'fandom', '{canonical}', '{canonical}', '2026-01-01T00:00:00Z', \
                         'curated', 0, 'browsable', 10)"
            ),
        )
        .await;
    }

    // `(fandom, window)` → how many works to tag at that instant.
    let plan = [
        (&emerging, "2026-01-05T00:00:00Z"),
        (&emerging, "2026-01-20T00:00:00Z"),
        // Exactly on the recent bound: `< recent_since` would file it under baseline and the split would
        // still total five, so only the per-window counts catch it.
        (&emerging, RECENT_SINCE),
        (&emerging, "2026-03-02T00:00:00Z"),
        (&emerging, "2026-03-10T00:00:00Z"),
        (&emerging, "2026-03-20T00:00:00Z"),
        (&quiet, "2026-01-03T00:00:00Z"),
        (&quiet, "2026-01-08T00:00:00Z"),
        (&quiet, "2026-01-13T00:00:00Z"),
        (&quiet, "2026-01-18T00:00:00Z"),
        (&quiet, "2026-01-25T00:00:00Z"),
        (&quiet, "2026-03-15T00:00:00Z"),
    ];
    for (node, added_at) in plan {
        let work = uuid::Uuid::new_v4().to_string();
        exec(
            db,
            &format!(
                "INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at) \
                 VALUES ('{work}', '{owner}', 'Work {work}', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
            ),
        )
        .await;
        // Raw insert rather than `taxonomy::tag_work`, because the arrival instant is the thing under
        // test and the store's own writer stamps `now`.
        exec(
            db,
            &format!(
                "INSERT INTO work_tags (work_id, node_id, prominence, source, role, added_at) \
                 VALUES ('{work}', '{node}', 10, 'imported', 'primary', '{added_at}')"
            ),
        )
        .await;
    }
}

#[tokio::test]
async fn the_windows_are_split_by_arrival_and_the_floor_is_applied_in_sql() {
    let (db, dir) = database("split").await;
    fixture(&db).await;

    let rows = darkhold_db::trend::counts(&db, BASELINE_SINCE, RECENT_SINCE, 3)
        .await
        .expect("counts");

    // The quiet fandom is real (6 works, 1 recent) and must not be returned: the rule will refuse to
    // judge it, so the store does not offer it for judging.
    assert_eq!(rows.len(), 1, "only the fandom above the floor: {rows:?}");
    assert_eq!(rows[0].fandom, "emerging-fandom");
    assert_eq!(
        rows[0].recent, 4,
        "four arrivals in March, including the one exactly on the bound"
    );
    assert_eq!(
        rows[0].baseline, 2,
        "two arrivals in January, February empty"
    );

    // With a floor of one, the quiet fandom appears with its own split — the same query, no second code
    // path, and it is ordered by recent arrivals rather than by name.
    let both = darkhold_db::trend::counts(&db, BASELINE_SINCE, RECENT_SINCE, 1)
        .await
        .expect("counts");
    assert_eq!(both.len(), 2, "{both:?}");
    let quiet = both
        .iter()
        .find(|row| row.fandom == "quiet-fandom")
        .expect("the quiet fandom at a floor of one");
    assert_eq!((quiet.recent, quiet.baseline), (1, 5), "{quiet:?}");

    let _ = std::fs::remove_dir_all(dir);
}
