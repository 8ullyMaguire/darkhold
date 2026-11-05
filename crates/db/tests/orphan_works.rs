//! M70-01 — orphaning a work: it survives, its author link does not, and deleting the account cannot take it.
//!
//! # What each assertion is for
//!
//! * **Survival** is the feature. A work that disappears with its author is the behaviour being replaced.
//! * **The label is frozen from the pseud, not the account.** A reader recognises `hollowgast`, not the login; a
//!   test that only checks "some label exists" passes for an implementation that wrote the account's email.
//! * **The original pseud is unreferenced.** This is what makes the account deletable, and it is the property the
//!   whole rekey exists for — an implementation that copies the work instead of moving it would pass a survival
//!   test and fail this one.
//! * **Deleting the account keeps the work.** The end-to-end claim, asserted by doing the deletion rather than by
//!   reasoning about the cascade.
//! * **Orphaning twice is a no-op**, because there is no owner left to detach.
//! * **Both engines.** The statements differ in casts and timestamps; the arm that has never run is the one that
//!   breaks.

use darkhold_db::Database;

fn scratch_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-orphan-{tag}-{}-{:?}",
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
                "lh_test_orphan_{}_{}",
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

async fn scalar(db: &Database, statement: &str) -> i64 {
    match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar(statement)
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("scalar"),
        darkhold_db::Backend::Postgres => sqlx::query_scalar(statement)
            .fetch_one(db.postgres_pool().expect("postgres"))
            .await
            .expect("scalar"),
    }
}

async fn text(db: &Database, statement: &str) -> Option<String> {
    match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar(statement)
            .fetch_optional(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("text"),
        darkhold_db::Backend::Postgres => sqlx::query_scalar(statement)
            .fetch_optional(db.postgres_pool().expect("postgres"))
            .await
            .expect("text"),
    }
}

/// One account, one pseud, one public work under that pseud.
///
/// The ids are UUID-shaped because PostgreSQL stores them as `uuid` and SQLite as `TEXT`: `'acc-1'` is fine on one
/// engine and a cast error on the other, which is how a test passes locally and fails the moment the other arm
/// runs.
async fn seed(db: &Database) {
    exec(
        db,
        "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version) \
         VALUES ('01955000-0000-4000-8000-000000000001', 'active', 'reader@example.invalid', 'adult', \
                 '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1)",
    )
    .await;
    exec(
        db,
        "INSERT INTO pseuds (id, account_id, handle, display_name, bio, discoverability, created_at, \
         updated_at, version) \
         VALUES ('01956000-0000-4000-8000-000000000001', '01955000-0000-4000-8000-000000000001', 'hollowgast', 'hollowgast', NULL, 'listed', \
                 '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1)",
    )
    .await;
    exec(
        db,
        "INSERT INTO works (id, owner_pseud_id, title, summary, language, rating, visibility, lifecycle, \
         completion, created_at, updated_at, version) \
         VALUES ('01957000-0000-4000-8000-000000000001', '01956000-0000-4000-8000-000000000001', 'The Long Way Round', '', 'en', 'general', 'public', 'published', \
                 'complete', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1)",
    )
    .await;
}

#[tokio::test]
async fn an_orphaned_work_survives_its_author_and_keeps_a_frozen_label() {
    let (db, dir) = database("survives").await;
    seed(&db).await;

    let label = darkhold_db::orphan::orphan_work(&db, "01957000-0000-4000-8000-000000000001")
        .await
        .expect("orphan")
        .expect("a work to orphan");
    assert_eq!(label, "hollowgast (orphaned)");

    // The work is still there, still published, still found by its title.
    assert_eq!(
        scalar(
            &db,
            "SELECT count(*) FROM works WHERE id = '01957000-0000-4000-8000-000000000001'"
        )
        .await,
        1
    );
    assert_eq!(
        text(
            &db,
            "SELECT title FROM works WHERE id = '01957000-0000-4000-8000-000000000001'"
        )
        .await
        .as_deref(),
        Some("The Long Way Round")
    );
    // Its author line is the frozen pseud and the label, not the account it was published from.
    assert_eq!(
        text(
            &db,
            "SELECT p.display_name FROM works w JOIN pseuds p ON p.id = w.owner_pseud_id \
             WHERE w.id = '01957000-0000-4000-8000-000000000001'",
        )
        .await
        .as_deref(),
        Some("hollowgast (orphaned)")
    );
    // The detached pseud belongs to the instance, not to a person. The cast is per engine: PostgreSQL stores the
    // id as UUID and SQLite as TEXT, and the `::text` in a shared string is exactly the kind of arm that never
    // runs on the other backend.
    let owner_sql = match db.backend() {
        darkhold_db::Backend::Sqlite => {
            "SELECT p.account_id FROM works w JOIN pseuds p ON p.id = w.owner_pseud_id WHERE w.id = '01957000-0000-4000-8000-000000000001'"
        }
        darkhold_db::Backend::Postgres => {
            "SELECT p.account_id::text FROM works w JOIN pseuds p ON p.id = w.owner_pseud_id WHERE w.id = '01957000-0000-4000-8000-000000000001'"
        }
    };
    assert_eq!(
        text(&db, owner_sql).await.as_deref(),
        Some(darkhold_db::SYSTEM_ACCOUNT.to_string().as_str())
    );

    // The original pseud now owns nothing, which is what lets its account be deleted at all.
    assert_eq!(
        scalar(&db, "SELECT count(*) FROM works WHERE owner_pseud_id = '01956000-0000-4000-8000-000000000001'").await,
        0,
        "a work that merely gained a second pseud would satisfy survival and fail this"
    );

    // And the deletion itself keeps the work.
    exec(
        &db,
        "DELETE FROM accounts WHERE id = '01955000-0000-4000-8000-000000000001'",
    )
    .await;
    assert_eq!(
        scalar(
            &db,
            "SELECT count(*) FROM works WHERE id = '01957000-0000-4000-8000-000000000001'"
        )
        .await,
        1,
        "the cascade must have nothing left to take"
    );
    assert_eq!(
        scalar(
            &db,
            "SELECT count(*) FROM pseuds WHERE id = '01956000-0000-4000-8000-000000000001'"
        )
        .await,
        0,
        "the author's pseud went with the account"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn orphaning_twice_is_a_no_op_rather_than_a_second_detachment() {
    let (db, dir) = database("twice").await;
    seed(&db).await;

    assert!(
        darkhold_db::orphan::orphan_work(&db, "01957000-0000-4000-8000-000000000001")
            .await
            .expect("first")
            .is_some()
    );
    assert_eq!(
        darkhold_db::orphan::orphan_work(&db, "01957000-0000-4000-8000-000000000001")
            .await
            .expect("second"),
        None,
        "there is no owner left to detach, and a second suffix would imply one"
    );
    assert_eq!(
        scalar(
            &db,
            "SELECT count(*) FROM pseuds WHERE handle LIKE 'orphaned-%'"
        )
        .await,
        1,
        "a second detachment would leave a second detached pseud behind"
    );
    // A missing work answers the same way: nothing here to detach.
    assert_eq!(
        darkhold_db::orphan::orphan_work(&db, "01957900-0000-4000-8000-00000000dead")
            .await
            .expect("missing"),
        None
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn erasing_an_account_orphans_its_works_instead_of_deleting_them() {
    let (db, dir) = database("erasure").await;
    seed(&db).await;

    // The whole point of the wiring: the caller erases a person, and the works outlive them.
    darkhold_db::erasure::erase_account(&db, "01955000-0000-4000-8000-000000000001")
        .await
        .expect("erase");

    assert_eq!(
        scalar(
            &db,
            "SELECT count(*) FROM accounts WHERE id = '01955000-0000-4000-8000-000000000001'"
        )
        .await,
        0,
        "the account is gone"
    );
    assert_eq!(
        scalar(
            &db,
            "SELECT count(*) FROM works WHERE id = '01957000-0000-4000-8000-000000000001'"
        )
        .await,
        1,
        "and the work is not"
    );
    assert_eq!(
        text(
            &db,
            "SELECT title FROM works WHERE id = '01957000-0000-4000-8000-000000000001'"
        )
        .await
        .as_deref(),
        Some("The Long Way Round"),
        "with its content, not a tombstone"
    );
    assert_eq!(
        scalar(&db, "SELECT count(*) FROM works WHERE owner_pseud_id = '01956000-0000-4000-8000-000000000001'").await,
        0,
        "the author's pseud left with the account; nothing points at it"
    );

    let _ = std::fs::remove_dir_all(dir);
}
