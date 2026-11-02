//! M47-08b §9.6: a reader's private search history, on both engines.
//!
//! Six properties, each one a way a history table goes wrong:
//!
//! 1. **A recorded search comes back**, newest first.
//! 2. **A repeated query is one row, moved to the front.** `Éowyn` and `  éowyn ` are the same
//!    query: the key is normalised in Rust precisely because SQLite's `lower()` folds ASCII only,
//!    and an expression index would make the two engines disagree about the length of a history.
//! 3. **The cap holds.** Twenty-one distinct queries leave twenty, and the oldest is the one gone.
//! 4. **A query too short to be a search is not recorded** — a keystroke is not a history entry.
//! 5. **Clearing removes this pseud's rows and reports how many**, and nothing else's.
//! 6. **One pseud cannot see another's history**, which is the whole privacy claim.
//!
//! Run on SQLite by default; set `DARKHOLD_TEST_PG_URL` for the PostgreSQL arm.

use std::time::Duration;

use darkhold_db::{search_history, Database};

fn make_config(url: String) -> darkhold_db::DatabaseConfig {
    darkhold_db::DatabaseConfig {
        url,
        max_connections: 2,
        acquire_timeout: Duration::from_secs(5),
        slow_query_warn: Duration::ZERO,
    }
}

/// A scratch database on whichever backend the selector names.
///
/// `search_history` carries no foreign key (see `0139`), so no account or pseud fixture is needed
/// — a pseud id is just a UUID here, and the isolation test can use two of them directly.
async fn connect(tag: &str) -> Database {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "darkhold-shist-{}-{}-{}",
        tag,
        uuid::Uuid::new_v4().simple(),
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let url = match std::env::var("DARKHOLD_TEST_PG_URL") {
        Ok(admin) => {
            let name = format!(
                "lh_shist_{}_{}",
                std::process::id(),
                uuid::Uuid::new_v4().simple()
            );
            let admin_db = Database::connect(&make_config(admin.clone()))
                .await
                .expect("connect to the admin database");
            sqlx::query(&format!("CREATE DATABASE {name}"))
                .execute(admin_db.postgres_pool().expect("postgres pool"))
                .await
                .expect("create a scratch database");
            admin_db.close().await;
            let (prefix, _) = admin
                .rsplit_once('/')
                .expect("admin URL ends in a database");
            format!("{prefix}/{name}")
        }
        Err(_) => format!("sqlite://{}/darkhold.sqlite?mode=rwc", dir.display()),
    };
    let db = Database::connect(&make_config(url)).await.expect("connect");
    db.migrate().await.expect("migrate");
    db
}

fn t(second: u32) -> String {
    format!("2026-11-02T00:{:02}:00Z", second)
}

#[tokio::test]
async fn a_recorded_search_comes_back_with_its_count() {
    let db = connect("record").await;
    let pseud = uuid::Uuid::new_v4();

    search_history::record(&db, pseud, "time travel", 12, &t(1), 20)
        .await
        .expect("record");

    let entries = search_history::recent(&db, pseud, 10)
        .await
        .expect("recent");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].query, "time travel");
    assert_eq!(entries[0].result_count, 12);
    assert_eq!(entries[0].searched_at, t(1));
}

#[tokio::test]
async fn a_repeated_query_is_one_row_moved_to_the_front() {
    let db = connect("repeat").await;
    let pseud = uuid::Uuid::new_v4();

    search_history::record(&db, pseud, "Time Travel", 3, &t(1), 20)
        .await
        .expect("record");
    search_history::record(&db, pseud, "another", 5, &t(2), 20)
        .await
        .expect("record");
    // Same query, different case and spacing, run again later.
    search_history::record(&db, pseud, "  time   TRAVEL ", 9, &t(3), 20)
        .await
        .expect("record");

    let entries = search_history::recent(&db, pseud, 10)
        .await
        .expect("recent");
    assert_eq!(
        entries.len(),
        2,
        "the repeat did not add a row: {entries:?}"
    );
    assert_eq!(entries[0].query, "time TRAVEL");
    assert_eq!(entries[0].result_count, 9, "the newer count wins");
    assert_eq!(entries[0].searched_at, t(3), "the newer search is first");
    assert_eq!(entries[1].query, "another");
}

#[tokio::test]
async fn the_cap_holds_and_the_oldest_entry_is_the_one_dropped() {
    let db = connect("cap").await;
    let pseud = uuid::Uuid::new_v4();

    for i in 0..21u32 {
        search_history::record(&db, pseud, &format!("query {i:02}"), i as i64, &t(i), 20)
            .await
            .expect("record");
    }

    let entries = search_history::recent(&db, pseud, 50)
        .await
        .expect("recent");
    assert_eq!(entries.len(), 20, "the cap is twenty rows, not twenty-one");
    assert_eq!(entries[0].query, "query 20", "newest first");
    assert!(
        !entries.iter().any(|e| e.query == "query 00"),
        "the oldest is the one trimmed: {entries:?}"
    );
    assert!(entries.iter().any(|e| e.query == "query 01"));
}

#[tokio::test]
async fn a_keystroke_is_not_a_history_entry() {
    let db = connect("short").await;
    let pseud = uuid::Uuid::new_v4();

    for q in ["", " ", "a", "  b  "] {
        search_history::record(&db, pseud, q, 0, &t(1), 20)
            .await
            .expect("record");
    }
    // Two characters is a query; one is a keystroke. The trim is what decides.
    search_history::record(&db, pseud, "ab", 1, &t(2), 20)
        .await
        .expect("record");

    let entries = search_history::recent(&db, pseud, 10)
        .await
        .expect("recent");
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0].query, "ab");
}

#[tokio::test]
async fn clearing_removes_only_this_pseud_and_reports_the_count() {
    let db = connect("clear").await;
    let mine = uuid::Uuid::new_v4();
    let theirs = uuid::Uuid::new_v4();

    for q in ["one", "two", "three"] {
        search_history::record(&db, mine, q, 1, &t(1), 20)
            .await
            .expect("record");
    }
    search_history::record(&db, theirs, "theirs", 1, &t(1), 20)
        .await
        .expect("record");

    let cleared = search_history::clear(&db, mine).await.expect("clear");
    assert_eq!(cleared, 3);
    assert!(search_history::recent(&db, mine, 10)
        .await
        .expect("recent")
        .is_empty());

    // Clearing twice reports zero rather than failing: the caller's question is "is it empty".
    assert_eq!(search_history::clear(&db, mine).await.expect("clear"), 0);
    assert_eq!(
        search_history::recent(&db, theirs, 10)
            .await
            .expect("recent")
            .len(),
        1,
        "another pseud's history is untouched"
    );
}

#[tokio::test]
async fn one_pseud_cannot_read_another_history() {
    let db = connect("isolation").await;
    let mine = uuid::Uuid::new_v4();
    let theirs = uuid::Uuid::new_v4();

    search_history::record(&db, mine, "private", 1, &t(1), 20)
        .await
        .expect("record");

    assert!(search_history::recent(&db, theirs, 10)
        .await
        .expect("recent")
        .is_empty());
}

#[test]
fn the_key_normalises_whitespace_and_case() {
    assert_eq!(search_history::query_key("  Time   TRAVEL "), "time travel");
    assert_eq!(search_history::query_key("Éowyn"), "éowyn");
    assert!(search_history::worth_recording("ab"));
    assert!(!search_history::worth_recording(" a "));
    assert!(!search_history::worth_recording("   "));
}
