//! M45-39 phase 2 — §13's account archive, at the store level and on both engines.
//!
//! The route test proves the document's shape; this proves the queries. Each section is a hand-written
//! SELECT against a table that has been renamed twice in this tree (`chapters`→`units`,
//! `account`→`account_id`), so the failure this catches is the one that compiles and dies at runtime.
//!
//! Run on SQLite by default; set `DARKHOLD_TEST_PG_URL` for the PostgreSQL arm.

use std::time::Duration;

use darkhold_db::archive;
use darkhold_db::Database;

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
        "darkhold-arch-{}-{}-{}",
        tag,
        uuid::Uuid::new_v4().simple(),
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let url = match std::env::var("DARKHOLD_TEST_PG_URL") {
        Ok(admin) => {
            let name = format!(
                "lh_arch_{}_{}",
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

struct Seeded {
    account: String,
    other: String,
}

/// Two accounts, each with a pseud, a work, a bookmark, a rating and a review.
///
/// Two rather than one: a section that forgot its `WHERE account_id` returns the other account's rows
/// and nothing else in this test would notice.
async fn seed(db: &Database) -> Seeded {
    let account = uuid::Uuid::new_v4().to_string();
    let other = uuid::Uuid::new_v4().to_string();
    // `is_public` is INTEGER on SQLite and BOOLEAN on PostgreSQL: the seed is a fixture, and a
    // fixture that only works on one engine would hide the difference the store has to handle.
    let flag = match db.backend() {
        darkhold_db::Backend::Sqlite => "1",
        darkhold_db::Backend::Postgres => "TRUE",
    };
    for (id, tag) in [(&account, "a"), (&other, "b")] {
        exec(
            db,
            &format!(
                "INSERT INTO accounts (id, email, status, age_state, created_at, updated_at, permission_statement)
                 VALUES ('{id}', 'arch-{tag}-{id}@example.test', 'active', 'adult', '{NOW_STAMP}', '{NOW_STAMP}', 'minimal')"
            ),
        )
        .await;
        let pseud = uuid::Uuid::new_v4().to_string();
        exec(
            db,
            &format!(
                "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at)
                 VALUES ('{pseud}', '{id}', 'arch-{tag}-{short}', 'Arch {tag}', '{NOW_STAMP}', '{NOW_STAMP}')",
                short = &pseud[..8]
            ),
        )
        .await;
        let work = uuid::Uuid::new_v4().to_string();
        exec(
            db,
            &format!(
                "INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at)
                 VALUES ('{work}', '{pseud}', 'Work {tag}', '{NOW_STAMP}', '{NOW_STAMP}')"
            ),
        )
        .await;
        exec(
            db,
            &format!(
                "INSERT INTO bookmarks (id, account_id, subject_type, subject_id, note, is_public, created_at, updated_at)
                 VALUES ('{}', '{id}', 'work', '{work}', 'note {tag}', {flag}, '{NOW_STAMP}', '{NOW_STAMP}')",
                uuid::Uuid::new_v4()
            ),
        )
        .await;
        exec(
            db,
            &format!(
                "INSERT INTO rating (id, account_id, pseud_id, work_id, stars, is_public, created_at, updated_at)
                 VALUES ('{}', '{id}', '{pseud}', '{work}', 4, {flag}, '{NOW_STAMP}', '{NOW_STAMP}')",
                uuid::Uuid::new_v4()
            ),
        )
        .await;
        exec(
            db,
            &format!(
                "INSERT INTO review (id, account_id, pseud_id, work_id, body, contains_spoilers, is_public, created_at, updated_at)
                 VALUES ('{}', '{id}', '{pseud}', '{work}', 'a review by {tag}', {flag}, {flag}, '{NOW_STAMP}', '{NOW_STAMP}')",
                uuid::Uuid::new_v4()
            ),
        )
        .await;
        exec(
            db,
            &format!(
                "INSERT INTO reading_status (id, account_id, subject_type, subject_id, status, updated_at)
                 VALUES ('{}', '{id}', 'work', '{work}', 'reading', '{NOW_STAMP}')",
                uuid::Uuid::new_v4()
            ),
        )
        .await;
    }
    Seeded { account, other }
}

fn section<'a>(sections: &'a [archive::Section], name: &str) -> &'a archive::Section {
    sections
        .iter()
        .find(|section| section.name == name)
        .unwrap_or_else(|| panic!("no section {name}"))
}

#[tokio::test]
async fn every_section_reads_without_a_sql_error_and_only_this_account() {
    let db = connect("sections").await;
    let seeded = seed(&db).await;

    let sections = archive::build(&db, &seeded.account).await.expect("build");

    // Every declared section exists, and the ones that are per-account carry exactly one row.
    let names: Vec<&str> = sections.iter().map(|section| section.name).collect();
    for required in [
        "account",
        "pseuds",
        "works",
        "bookmarks",
        "library",
        "reading_status",
        "reading_progress",
        "reading_history",
        "ratings",
        "reviews",
        "credits",
    ] {
        assert!(
            names.contains(&required),
            "missing section {required}: {names:?}"
        );
    }

    assert_eq!(section(&sections, "account").count, 1);
    assert_eq!(section(&sections, "pseuds").count, 1, "one pseud, not two");
    assert_eq!(section(&sections, "works").count, 1, "her work, not his");
    assert_eq!(section(&sections, "bookmarks").count, 1);
    assert_eq!(section(&sections, "ratings").count, 1);
    assert_eq!(section(&sections, "reviews").count, 1);
    assert_eq!(section(&sections, "reading_status").count, 1);

    // And the other account's archive is its own.
    let theirs = archive::build(&db, &seeded.other)
        .await
        .expect("build theirs");
    assert_eq!(section(&theirs, "bookmarks").count, 1);
    assert_ne!(
        section(&sections, "account").rows[0]["id"],
        section(&theirs, "account").rows[0]["id"],
        "two accounts, two archives"
    );
}

#[tokio::test]
async fn a_checksum_changes_when_the_rows_do() {
    let db = connect("checksums").await;
    let seeded = seed(&db).await;

    let flag = match db.backend() {
        darkhold_db::Backend::Sqlite => "1",
        darkhold_db::Backend::Postgres => "TRUE",
    };
    let before = archive::build(&db, &seeded.account).await.expect("build");
    let bookmarks_before = section(&before, "bookmarks").sha256.clone();

    exec(
        &db,
        &format!(
            "INSERT INTO bookmarks (id, account_id, subject_type, subject_id, note, is_public, created_at, updated_at)
             VALUES ('{}', '{}', 'work', '{}', 'a second thought', {flag}, '{NOW_STAMP}', '{NOW_STAMP}')",
            uuid::Uuid::new_v4(),
            seeded.account,
            uuid::Uuid::new_v4()
        ),
    )
    .await;

    let after = archive::build(&db, &seeded.account)
        .await
        .expect("build again");
    assert_eq!(section(&after, "bookmarks").count, 2);
    assert_ne!(
        section(&after, "bookmarks").sha256,
        bookmarks_before,
        "the hash follows the rows, which is the only reason to publish it"
    );
    // A section nobody touched keeps its hash, so a reader can diff two archives.
    assert_eq!(
        section(&after, "ratings").sha256,
        section(&before, "ratings").sha256
    );
}

#[tokio::test]
async fn an_erased_account_has_an_archive_of_nothing() {
    let db = connect("erased").await;
    let seeded = seed(&db).await;

    // Soft-delete everything this account owns, the way §58's erasure does.
    exec(
        &db,
        &format!(
            "UPDATE pseuds SET deleted_at = '{NOW_STAMP}' WHERE account_id = '{}'",
            seeded.account
        ),
    )
    .await;
    exec(
        &db,
        &format!(
            "UPDATE rating SET deleted_at = '{NOW_STAMP}' WHERE account_id = '{}'",
            seeded.account
        ),
    )
    .await;
    exec(
        &db,
        &format!(
            "UPDATE review SET deleted_at = '{NOW_STAMP}' WHERE account_id = '{}'",
            seeded.account
        ),
    )
    .await;

    let sections = archive::build(&db, &seeded.account).await.expect("build");
    assert_eq!(
        section(&sections, "pseuds").count,
        0,
        "an archive of erased data would be an erasure that did not happen"
    );
    assert_eq!(section(&sections, "works").count, 0);
    assert_eq!(section(&sections, "ratings").count, 0);
    assert_eq!(section(&sections, "reviews").count, 0);
}
