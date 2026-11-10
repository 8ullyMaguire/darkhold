//! M47-13 §9.7.9: reading goals, on both engines.
//!
//! What is asserted here is what the feature would get wrong quietly:
//!
//! 1. **A goal is opt-in by existing** — no row, no goal, and clearing leaves none.
//! 2. **Setting twice replaces** rather than creating a second goal for the same pseud.
//! 3. **The chapters metric counts distinct chapters**, so re-opening one chapter five times is one.
//! 4. **The words metric sums those chapters' current revisions**, not every chapter in the work.
//! 5. **The works metric is account-scoped and counts only `finished_at` in the window**, which is
//!    the one metric whose scope is not the pseud.
//!
//! Run on SQLite by default; set `DARKHOLD_TEST_PG_URL` for the PostgreSQL arm.

use std::time::Duration;

use darkhold_db::reading::{current_revision_of_unit, save_progress, ProgressInput};
use darkhold_db::reading_goal as store;
use darkhold_db::Database;
use darkhold_domain::reading_goal::{Goal, Metric, Period};
use time::macros::datetime;

const NOW_STAMP: &str = "2026-11-02T10:00:00Z";

fn make_config(url: String) -> darkhold_db::DatabaseConfig {
    darkhold_db::DatabaseConfig {
        url,
        max_connections: 2,
        acquire_timeout: Duration::from_secs(5),
        slow_query_warn: Duration::ZERO,
    }
}

async fn connect(tag: &str) -> Database {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "darkhold-rg-{}-{}-{}",
        tag,
        uuid::Uuid::new_v4().simple(),
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let url = match std::env::var("DARKHOLD_TEST_PG_URL") {
        Ok(admin) => {
            let name = format!(
                "lh_rg_{}_{}",
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

async fn exec(db: &Database, query: &str) {
    let q = db.sql(query, query);
    match db.backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(&q)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("exec");
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(&q)
                .execute(db.postgres_pool().expect("pg"))
                .await
                .expect("exec");
        }
    }
}

struct Reader {
    account: String,
    pseud: String,
}

/// An account and its pseud.
async fn a_reader(db: &Database) -> Reader {
    let account = uuid::Uuid::new_v4().to_string();
    exec(
        db,
        &format!(
            "INSERT INTO accounts (id, email, status, age_state, created_at, updated_at, permission_statement)
             VALUES ('{account}', '{account}@example.test', 'active', 'adult', '{NOW_STAMP}', '{NOW_STAMP}', 'minimal')"
        ),
    )
    .await;
    let pseud = uuid::Uuid::new_v4().to_string();
    exec(
        db,
        &format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at)
             VALUES ('{pseud}', '{account}', 'rg-{}', 'RG', '{NOW_STAMP}', '{NOW_STAMP}')",
            &pseud[..8]
        ),
    )
    .await;
    Reader { account, pseud }
}

/// A published work with `chapters` units (the schema's `units` table), each `words` long.
///
/// The reader-facing word is "chapter"; the tables are `units` and `revisions` since the rename,
/// which is why this fixture writes `units`.
async fn work(db: &Database, pseud: &str, chapters: usize, words: i64) -> (String, Vec<String>) {
    let work_id = uuid::Uuid::new_v4().to_string();
    exec(
        db,
        &format!(
            "INSERT INTO works (id, owner_pseud_id, title, lifecycle, visibility, created_at, updated_at)
             VALUES ('{work_id}', '{pseud}', 'A Work', 'published', 'public', '{NOW_STAMP}', '{NOW_STAMP}')"
        ),
    )
    .await;
    let mut ids = Vec::new();
    for index in 0..chapters {
        let chapter = uuid::Uuid::new_v4().to_string();
        let revision = uuid::Uuid::new_v4().to_string();
        exec(
            db,
            &format!(
                "INSERT INTO units (id, work_id, order_key, title, created_at, updated_at)
                 VALUES ('{chapter}', '{work_id}', {order}, 'Chapter', '{NOW_STAMP}', '{NOW_STAMP}')",
                order = (index as i64 + 1) * 10
            ),
        )
        .await;
        exec(
            db,
            &format!(
                "INSERT INTO revisions
                 (id, unit_id, revision_number, document_json, sanitized_html, plain_text, word_count, created_by_pseud_id, created_at)
                 VALUES ('{revision}', '{chapter}', 1, '{{}}', '<p>x</p>', 'x', {words}, '{pseud}', '{NOW_STAMP}')"
            ),
        )
        .await;
        exec(
            db,
            &format!("UPDATE units SET current_revision_id = '{revision}' WHERE id = '{chapter}'"),
        )
        .await;
        ids.push(chapter);
    }
    (work_id, ids)
}

/// One chapter read by `reader` at `at`, through the store the route writes.
///
/// Deliberately through `save_progress` rather than raw SQL, and through the same entry point the progress route runs: the
/// chapter record is written in the position's transaction, so a fixture that inserted it itself could pass while the route
/// silently failed to. `reading_goal::record_unit_read` used to be called here; it was a second writer of the same fact with no
/// transaction to share, and deleting it is what makes this fixture prove something.
async fn read_chapter(db: &Database, reader: &Reader, work: &str, chapter: &str, at: &str) {
    let account = reader.account.parse().expect("an account id");
    let pseud = reader.pseud.parse().expect("a pseud id");
    let revision = current_revision_of_unit(db, chapter)
        .await
        .expect("the chapter's current revision");

    save_progress(
        db,
        ProgressInput {
            account,
            pseud: Some(pseud),
            subject_type: "work",
            subject_id: work,
            unit_id: Some(chapter),
            revision,
            anchor: None,
            fraction: 1000,
            device: None,
        },
    )
    .await
    .expect("record the chapter read");

    // The writer stamps the clock it has; the fixture needs a read that happened at a stated instant, because every goal is read
    // against a window. Setting the two timestamps here keeps the product path the one that WROTE the row while the test owns
    // the time - the alternative, teaching `save_progress` a test-only timestamp, would put a fixture concern in the product.
    exec(
        db,
        &format!(
            "UPDATE reading_unit_read SET first_read_at = '{at}', last_read_at = '{at}' \
             WHERE unit_id = '{chapter}'"
        ),
    )
    .await;
}

fn daily(metric: Metric, target: i64) -> Goal {
    Goal {
        period: Period::Daily,
        metric,
        target,
    }
}

#[tokio::test]
async fn a_goal_is_opt_in_by_existing_and_clearing_removes_it() {
    let db = connect("optin").await;
    let reader = a_reader(&db).await;

    assert!(
        store::get(&db, &reader.pseud).await.expect("get").is_none(),
        "no row, no goal"
    );
    store::set(&db, &reader.pseud, daily(Metric::Chapters, 3), NOW_STAMP)
        .await
        .expect("set");
    assert_eq!(
        store::get(&db, &reader.pseud).await.expect("get"),
        Some(daily(Metric::Chapters, 3))
    );

    assert!(store::clear(&db, &reader.pseud).await.expect("clear"));
    assert!(!store::clear(&db, &reader.pseud).await.expect("clear twice"));
    assert!(store::get(&db, &reader.pseud).await.expect("get").is_none());
}

#[tokio::test]
async fn setting_twice_replaces_the_goal_rather_than_adding_one() {
    let db = connect("replace").await;
    let reader = a_reader(&db).await;

    store::set(&db, &reader.pseud, daily(Metric::Chapters, 3), NOW_STAMP)
        .await
        .expect("set");
    store::set(&db, &reader.pseud, daily(Metric::Words, 5000), NOW_STAMP)
        .await
        .expect("replace");

    assert_eq!(
        store::get(&db, &reader.pseud).await.expect("get"),
        Some(daily(Metric::Words, 5000))
    );
}

#[tokio::test]
async fn the_chapters_metric_counts_distinct_chapters_in_the_window() {
    let db = connect("chapters").await;
    let reader = a_reader(&db).await;
    let author = a_reader(&db).await;
    let (work_id, chapters) = work(&db, &author.pseud, 3, 100).await;

    // Two chapters read today, one of them twice, one read yesterday.
    read_chapter(&db, &reader, &work_id, &chapters[0], "2026-11-02T09:00:00Z").await;
    read_chapter(&db, &reader, &work_id, &chapters[0], "2026-11-02T11:00:00Z").await;
    read_chapter(&db, &reader, &work_id, &chapters[1], "2026-11-02T12:00:00Z").await;
    read_chapter(&db, &reader, &work_id, &chapters[2], "2026-11-01T12:00:00Z").await;

    let progress = store::progress(
        &db,
        daily(Metric::Chapters, 4),
        &reader.pseud,
        &reader.account,
        datetime!(2026-11-02 13:00:00 UTC),
    )
    .await
    .expect("progress");

    assert_eq!(
        progress.current, 2,
        "one chapter counted once, yesterday excluded"
    );
    assert_eq!(progress.window_start, "2026-11-02T00:00:00Z");
    assert_eq!(progress.window_end, "2026-11-03T00:00:00Z");
    assert!(!progress.met());
    assert_eq!(progress.remaining(), 2);
}

#[tokio::test]
async fn the_words_metric_sums_the_chapters_actually_read() {
    let db = connect("words").await;
    let reader = a_reader(&db).await;
    let author = a_reader(&db).await;
    let (work_id, chapters) = work(&db, &author.pseud, 3, 1200).await;

    read_chapter(&db, &reader, &work_id, &chapters[0], "2026-11-02T09:00:00Z").await;
    read_chapter(&db, &reader, &work_id, &chapters[1], "2026-11-02T10:00:00Z").await;

    let progress = store::progress(
        &db,
        daily(Metric::Words, 10_000),
        &reader.pseud,
        &reader.account,
        datetime!(2026-11-02 13:00:00 UTC),
    )
    .await
    .expect("progress");

    assert_eq!(
        progress.current, 2400,
        "two of three chapters, 1200 words each"
    );
    assert_eq!(progress.remaining(), 7600);
}

#[tokio::test]
async fn the_works_metric_reads_the_account_shelf_and_its_window() {
    let db = connect("works").await;
    let reader = a_reader(&db).await;
    let author = a_reader(&db).await;
    let (work_a, _) = work(&db, &author.pseud, 1, 10).await;
    let (work_b, _) = work(&db, &author.pseud, 1, 10).await;

    // One finished today, one finished yesterday.
    for (work, finished) in [
        (&work_a, "2026-11-02T08:00:00Z"),
        (&work_b, "2026-11-01T08:00:00Z"),
    ] {
        exec(
            &db,
            &format!(
                "INSERT INTO reading_status (id, account_id, subject_type, subject_id, status, finished_at, updated_at, version)
                 VALUES ('{}', '{}', 'work', '{work}', 'finished', '{finished}', '{finished}', 1)",
                uuid::Uuid::new_v4(),
                reader.account
            ),
        )
        .await;
    }

    let progress = store::progress(
        &db,
        daily(Metric::Works, 2),
        &reader.pseud,
        &reader.account,
        datetime!(2026-11-02 13:00:00 UTC),
    )
    .await
    .expect("progress");

    assert_eq!(progress.current, 1, "only today's finished work counts");
    assert_eq!(
        Metric::Works.scope(),
        "account",
        "the payload must say this metric is not per-pseud"
    );
}
