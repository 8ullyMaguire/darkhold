//! M45-76 Step 3.1: the sweep's candidate query on both engines.
//!
//! The load-bearing case is an imported work: its publication event is an
//! import artifact and must not mask `library_items.source_updated_at`. A
//! hosted work keeps the event clock. Both are checked here rather than in a
//! later sweep, so a query that silently reports nothing eligible fails.

use darkhold_db::work_lifecycle::{apply_transition, candidates, Candidate, CompletionSource};
use std::path::PathBuf;

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-wl-candidates-{tag}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

async fn exec(db: &darkhold_db::Database, sql: &str) {
    match db.backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(sql)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await
                .unwrap_or_else(|e| panic!("fixture on SQLite: {sql}: {e}"));
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(sql)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await
                .unwrap_or_else(|e| panic!("fixture on PostgreSQL: {sql}: {e}"));
        }
    }
}

#[tokio::test]
async fn candidates_keep_the_author_clock_and_report_source_completion() {
    let tdb = test_support::TestDb::connect_with_dir("wl-candidates", &scratch("fixture")).await;
    let db = tdb.db();

    exec(db, "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version) VALUES ('01957600-0000-4000-8000-00000000a001','active','wl@test.dev','adult','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)").await;
    exec(db, "INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at, updated_at, version) VALUES ('01957600-0000-4000-8000-00000000b001','01957600-0000-4000-8000-00000000a001','wl','WL','listed','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)").await;

    for (n, completion) in [
        ("01", "in_progress"),
        ("02", "in_progress"),
        ("03", "in_progress"),
        ("04", "in_progress"),
        ("05", "hiatus"),
        ("06", "complete"),
    ] {
        let sql = format!(
            "INSERT INTO works (id, owner_pseud_id, title, visibility, lifecycle, completion, created_at, updated_at) \
             VALUES ('01957600-0000-4000-8000-0000000000{n}','01957600-0000-4000-8000-00000000b001','Work {n}','public','published','{completion}','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')"
        );
        exec(db, &sql).await;
    }

    // 03 has only the naive published_at clock.
    exec(db, "UPDATE works SET published_at = '2025-01-01T00:00:00' WHERE id = '01957600-0000-4000-8000-000000000003'").await;

    // 01 is imported: its publish event is an import artifact and must be masked.
    exec(db, "INSERT INTO library_items (id, account_id, work_id, source_key, source_work_key, title, status, source_updated_at, source_url, created_at, updated_at) VALUES ('01957600-0000-4000-8000-000000001001','01957600-0000-4000-8000-00000000a001','01957600-0000-4000-8000-000000000001','wl','w1','One','ongoing','2026-01-01T00:00:00Z','https://example.invalid/1','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')").await;
    exec(db, "INSERT INTO publication_events (id, work_id, action, occurred_at, actor_pseud_id) VALUES ('01957600-0000-4000-8000-000000002001','01957600-0000-4000-8000-000000000001','update','2026-10-01T00:00:00Z','01957600-0000-4000-8000-00000000b001')").await;

    // 02 is hosted: the event clock is the only clock.
    exec(db, "INSERT INTO publication_events (id, work_id, action, occurred_at, actor_pseud_id) VALUES ('01957600-0000-4000-8000-000000002002','01957600-0000-4000-8000-000000000002','update','2026-08-01T00:00:00Z','01957600-0000-4000-8000-00000000b001')").await;

    // 04 says complete at source; 05 has two library items and the later clock wins.
    exec(db, "INSERT INTO library_items (id, account_id, work_id, source_key, source_work_key, title, status, source_url, created_at, updated_at) VALUES ('01957600-0000-4000-8000-000000001004','01957600-0000-4000-8000-00000000a001','01957600-0000-4000-8000-000000000004','wl','w4','Four','complete','https://example.invalid/4','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')").await;
    for (n, at) in [
        ("1005", "2026-02-01T00:00:00Z"),
        ("1006", "2026-03-01T00:00:00Z"),
    ] {
        let sql = format!(
            "INSERT INTO library_items (id, account_id, work_id, source_key, source_work_key, title, status, source_updated_at, source_url, created_at, updated_at) \
             VALUES ('01957600-0000-4000-8000-00000000{n}','01957600-0000-4000-8000-00000000a001','01957600-0000-4000-8000-000000000005','wl','w5{n}','Five','hiatus','{at}','https://example.invalid/5','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')"
        );
        exec(db, &sql).await;
    }

    let rows: Vec<Candidate> = candidates(db, None, 10).await.expect("candidate query");
    let find = |tail: &str| -> &Candidate {
        rows.iter()
            .find(|c| c.work_id.ends_with(tail))
            .unwrap_or_else(|| panic!("missing candidate {tail}: {rows:?}"))
    };

    assert_eq!(find("0001").last_event, None, "import event must be masked");
    assert_eq!(
        find("0001").last_source.as_deref(),
        Some("2026-01-01T00:00:00Z")
    );
    assert_eq!(
        find("0002").last_event.as_deref(),
        Some("2026-08-01T00:00:00Z")
    );
    assert_eq!(
        find("0003").published_at.as_deref(),
        Some("2025-01-01T00:00:00")
    );
    assert!(find("0004").source_says_complete);
    assert_eq!(
        find("0005").last_source.as_deref(),
        Some("2026-03-01T00:00:00Z")
    );
    assert!(
        !rows.iter().any(|c| c.work_id.ends_with("0006")),
        "complete work is not a candidate"
    );
    assert!(rows.iter().all(|c| c.completion_source == "import"));

    tdb.cleanup().await;
}

async fn scalar(db: &darkhold_db::Database, sqlite: &str, postgres: &str, param: &str) -> i64 {
    let sql = db.sql(sqlite, postgres);
    match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar(&sql)
            .bind(param)
            .fetch_one(db.sqlite_pool().expect("sqlite handle"))
            .await
            .expect("scalar on SQLite"),
        darkhold_db::Backend::Postgres => sqlx::query_scalar(&sql)
            .bind(param)
            .fetch_one(db.postgres_pool().expect("postgres handle"))
            .await
            .expect("scalar on PostgreSQL"),
    }
}

#[tokio::test]
async fn apply_transition_is_conditional_and_writes_one_mark() {
    let tdb = test_support::TestDb::connect_with_dir("wl-transition", &scratch("transition")).await;
    let db = tdb.db();
    let work = "01957600-0000-4000-8000-000000000007";
    let actor = "01957600-0000-4000-8000-00000000a001";

    exec(db, "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version) VALUES ('01957600-0000-4000-8000-00000000a001','active','wl@test.dev','adult','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)").await;
    exec(db, "INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at, updated_at, version) VALUES ('01957600-0000-4000-8000-00000000b001','01957600-0000-4000-8000-00000000a001','wl','WL','listed','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)").await;
    exec(db, "INSERT INTO works (id, owner_pseud_id, title, visibility, lifecycle, completion, created_at, updated_at) VALUES ('01957600-0000-4000-8000-000000000007','01957600-0000-4000-8000-00000000b001','Seven','public','published','in_progress','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')").await;

    let first = apply_transition(
        db,
        work,
        "in_progress",
        "abandoned",
        CompletionSource::Sweep,
        Some(actor),
        Some("stale"),
        Some("2026-01-01T00:00:00Z"),
        "2026-11-24T00:00:00Z",
    )
    .await
    .expect("first transition");
    assert!(
        first,
        "the conditional update must match the current completion"
    );

    let second = apply_transition(
        db,
        work,
        "in_progress",
        "abandoned",
        CompletionSource::Sweep,
        Some(actor),
        Some("stale"),
        Some("2026-01-01T00:00:00Z"),
        "2026-11-24T00:00:00Z",
    )
    .await
    .expect("second transition");
    assert!(!second, "a stale `from` loses the race without an error");

    let marks: i64 = scalar(
        db,
        "SELECT count(*) FROM work_completion_marks WHERE work_id = ?",
        "SELECT count(*) FROM work_completion_marks WHERE work_id = ?::uuid",
        work,
    )
    .await;
    assert_eq!(marks, 1, "one transition writes exactly one history row");

    let state: (String, String, String) = match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_as(
            "SELECT completion, completion_source, completion_marked_at FROM works WHERE id = ?",
        )
        .bind(work)
        .fetch_one(db.sqlite_pool().expect("sqlite handle"))
        .await
        .expect("read work state"),
        darkhold_db::Backend::Postgres => sqlx::query_as(
            "SELECT completion, completion_source, completion_marked_at FROM works WHERE id = $1::uuid",
        )
        .bind(work)
        .fetch_one(db.postgres_pool().expect("postgres handle"))
        .await
        .expect("read work state"),
    };
    assert_eq!(state.0, "abandoned");
    assert_eq!(state.1, "sweep");
    assert_eq!(state.2, "2026-11-24T00:00:00Z");

    let stored_actor: Option<String> = match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar(
            "SELECT actor_account_id FROM work_completion_marks WHERE work_id = ?",
        )
        .bind(work)
        .fetch_one(db.sqlite_pool().expect("sqlite handle"))
        .await
        .expect("read mark actor"),
        darkhold_db::Backend::Postgres => sqlx::query_scalar(
            "SELECT actor_account_id::text FROM work_completion_marks WHERE work_id = $1::uuid",
        )
        .bind(work)
        .fetch_one(db.postgres_pool().expect("postgres handle"))
        .await
        .expect("read mark actor"),
    };
    assert_eq!(
        stored_actor.as_deref(),
        Some(actor),
        "actor must round-trip"
    );

    tdb.cleanup().await;
}
