//! M64-03 — a day's per-arm numbers are computed once and then read.
//!
//! Three properties, and each one is a way the naive version fails:
//!
//! 1. **the join is the definition** — a slot with no arm assignment is not an observation of an arm, so it must not
//!    inflate any arm's count;
//! 2. **a re-run repairs, it does not double** — the job will be run twice the first week there is a bug upstream;
//! 3. **guest traffic is stored** — the guardrail is "one bot cannot decide a winner", and it has to be answerable from
//!    the snapshot a promotion read, not from a log that has moved on since.

use darkhold_db::Database;

const ACCOUNT: &str = "01955000-0000-4000-8000-0000000001a1";
const GUEST_ACCOUNT: &str = "01955000-0000-4000-8000-0000000001a2";
const PSEUD: &str = "01955000-0000-4000-8000-0000000001b1";
const GUEST_PSEUD: &str = "01955000-0000-4000-8000-0000000001b2";
const UNASSIGNED_PSEUD: &str = "01955000-0000-4000-8000-0000000001b3";
const ACCOUNT3: &str = "01955000-0000-4000-8000-0000000001a3";
const WORK: &str = "01957000-0000-4000-8000-0000000001c1";

fn scratch_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-snapshot-{tag}-{}-{:?}",
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
                "lh_test_snap_{}_{}",
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

/// Two accounts — one a guest — and three pseuds: two assigned to an arm, one deliberately not.
async fn seed(db: &Database) {
    for (account, email, guest) in [
        (ACCOUNT, "snap@mfl.invalid", 0),
        (GUEST_ACCOUNT, "guest@mfl.invalid", 1),
        (ACCOUNT3, "unassigned@mfl.invalid", 0),
    ] {
        exec(
            db,
            &format!(
                "INSERT INTO accounts (id, status, email, age_state, is_guest, created_at, updated_at, version) \
                 VALUES ('{account}', 'active', '{email}', 'adult', {guest}, \
                         '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1)"
            ),
        )
        .await;
    }
    for (pseud, account, handle) in [
        (PSEUD, ACCOUNT, "snap"),
        (GUEST_PSEUD, GUEST_ACCOUNT, "guest"),
        (UNASSIGNED_PSEUD, ACCOUNT3, "unassigned"),
    ] {
        exec(
            db,
            &format!(
                "INSERT INTO pseuds (id, account_id, handle, display_name, bio, discoverability, created_at, \
                 updated_at, version) VALUES ('{pseud}', '{account}', '{handle}', '{handle}', NULL, 'listed', \
                 '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1)"
            ),
        )
        .await;
    }
    exec(
        db,
        &format!(
            "INSERT INTO works (id, owner_pseud_id, title, summary, language, rating, visibility, lifecycle, \
             completion, created_at, updated_at, version) VALUES ('{WORK}', '{PSEUD}', 'Snapped', '', 'en', \
             'general', 'public', 'published', 'complete', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1)"
        ),
    )
    .await;
    // Two arms on the same surface: the reader in arm 0, the guest in arm 1, the third with no assignment at all.
    for (pseud, arm) in [(PSEUD, 0), (GUEST_PSEUD, 1)] {
        exec(
            db,
            &format!(
                "INSERT INTO experiment_assignments (id, pseud_id, surface_id, bucket, assigned_at, version) \
                 VALUES ('01955000-0000-4000-8000-0000000001d{arm}', '{pseud}', 'discover.feed', {arm}, \
                 '2026-01-01T00:00:00Z', 1)"
            ),
        )
        .await;
    }
}

async fn slot(db: &Database, id: &str, pseud: &str, chosen: bool) {
    let chosen_at = if chosen { "datetime('now')" } else { "NULL" };
    exec(
        db,
        &format!(
            "INSERT INTO recommendation_slots \
             (id, pseud_id, work_id, request_id, position, reasons, mechanism, created_at, chosen_at) \
             VALUES ('{id}', '{pseud}', '{WORK}', 'req-{id}', 1, '[]', 'discover.feed', \
                     '2026-11-06T10:00:00Z', {chosen_at})"
        ),
    )
    .await;
}

#[tokio::test]
async fn a_day_counts_only_slots_that_have_an_arm() {
    let (db, dir) = database("arms").await;
    seed(&db).await;
    slot(&db, "01955000-0000-4000-8000-0000000001e1", PSEUD, true).await;
    slot(&db, "01955000-0000-4000-8000-0000000001e2", PSEUD, false).await;
    slot(&db, "01955000-0000-4000-8000-0000000001e3", GUEST_PSEUD, false).await;
    // Served to a reader with no assignment: not an observation of any arm.
    slot(&db, "01955000-0000-4000-8000-0000000001e4", UNASSIGNED_PSEUD, false).await;

    let written = darkhold_db::experiment_snapshots::snapshot_day(&db, "2026-11-06")
        .await
        .expect("snapshot");
    assert_eq!(written, 2, "two arms had traffic; the unassigned slot belongs to neither");

    let rows = darkhold_db::experiment_snapshots::day_snapshots(&db, "2026-11-06")
        .await
        .expect("read back");
    assert_eq!(rows.len(), 2, "{rows:?}");
    let arm0 = rows.iter().find(|r| r.arm == 0).expect("arm 0");
    assert_eq!(arm0.impressions, 2, "both of this reader's slots");
    assert_eq!(arm0.readers, 1, "one reader, two impressions");
    assert_eq!(arm0.events, 1, "one slot was followed by the choice event");
    assert_eq!(arm0.guest_readers, 0);

    let arm1 = rows.iter().find(|r| r.arm == 1).expect("arm 1");
    assert_eq!(arm1.impressions, 1);
    assert_eq!(arm1.guest_readers, 1, "the guardrail input: this arm's only reader is a guest");

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_second_run_repairs_the_day_rather_than_doubling_it() {
    let (db, dir) = database("rerun").await;
    seed(&db).await;
    slot(&db, "01955000-0000-4000-8000-0000000001f1", PSEUD, false).await;

    darkhold_db::experiment_snapshots::snapshot_day(&db, "2026-11-06")
        .await
        .expect("first run");
    darkhold_db::experiment_snapshots::snapshot_day(&db, "2026-11-06")
        .await
        .expect("second run");

    let rows = darkhold_db::experiment_snapshots::day_snapshots(&db, "2026-11-06")
        .await
        .expect("read back");
    let arm0 = rows.iter().find(|r| r.arm == 0).expect("arm 0");
    assert_eq!(arm0.impressions, 1, "one impression, however many times the job runs");

    // A late slot for the same day: the re-run must absorb it rather than adding a second row.
    slot(&db, "01955000-0000-4000-8000-0000000001f2", PSEUD, false).await;
    darkhold_db::experiment_snapshots::snapshot_day(&db, "2026-11-06")
        .await
        .expect("repair run");
    let rows = darkhold_db::experiment_snapshots::day_snapshots(&db, "2026-11-06")
        .await
        .expect("read back");
    assert_eq!(
        rows.len(),
        1,
        "one row for the arm that has traffic, and no second row for the arm that has none: {rows:?}"
    );
    let arm0 = rows.iter().find(|r| r.arm == 0).expect("arm 0");
    assert_eq!(arm0.impressions, 2, "the repair included the late slot");

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn no_snapshot_is_not_an_empty_snapshot() {
    let (db, dir) = database("empty").await;
    seed(&db).await;

    let latest = darkhold_db::experiment_snapshots::latest_snapshots(&db)
        .await
        .expect("read");
    assert!(
        latest.is_none(),
        "nothing has been computed, which the route reports as null rather than []"
    );

    let _ = std::fs::remove_dir_all(dir);
}
