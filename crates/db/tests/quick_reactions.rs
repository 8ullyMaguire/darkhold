//! M47-12 §9.4: the labelled one-click reaction bar, on both engines.
//!
//! Five properties, each one a way this table goes wrong:
//!
//! 1. **One row per reader per label**, so re-reacting is idempotent and two readers are two.
//! 2. **Labels are independent**: one reader may hold several on the same work, which is why this
//!    cannot share `work_reactions`' `(work, pseud)` key.
//! 3. **The tally is public and ordered**, most-reacted first, so two reads agree.
//! 4. **Withdrawing is silent and idempotent** — it reports whether there was anything to remove
//!    rather than failing.
//! 5. **The author's feedback preference gates the bar**, work override first, then the owner's
//!    account default, then enabled.
//!
//! Run on SQLite by default; set `DARKHOLD_TEST_PG_URL` for the PostgreSQL arm.

use std::time::Duration;

use darkhold_db::{quick_reactions, Database};

const NOW: &str = "2026-11-02T00:00:00Z";

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
        "darkhold-qr-{}-{}-{}",
        tag,
        uuid::Uuid::new_v4().simple(),
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let url = match std::env::var("DARKHOLD_TEST_PG_URL") {
        Ok(admin) => {
            let name = format!(
                "lh_qr_{}_{}",
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

/// A boolean literal for the backend in play.
///
/// `feedback_preferences.comments_enabled` is `INTEGER 0/1` on SQLite and `BOOLEAN` on PostgreSQL,
/// and PostgreSQL rejects an integer literal for a boolean column with "column is of type boolean
/// but expression is of type integer". The fixtures write the literal the engine accepts instead of
/// casting, so they stay readable as the schema they are filling.
fn boolean(db: &Database, value: bool) -> &'static str {
    match (db.backend(), value) {
        (darkhold_db::Backend::Sqlite, false) => "0",
        (darkhold_db::Backend::Sqlite, true) => "1",
        (darkhold_db::Backend::Postgres, false) => "false",
        (darkhold_db::Backend::Postgres, true) => "true",
    }
}

/// An account and its pseud, returning `(account_id, pseud_id)`.
async fn reader(db: &Database) -> (String, String) {
    let account = uuid::Uuid::new_v4().to_string();
    exec(
        db,
        &format!(
            "INSERT INTO accounts (id, email, status, age_state, created_at, updated_at, permission_statement)
             VALUES ('{account}', '{account}@example.test', 'active', 'adult', '{NOW}', '{NOW}', 'minimal')"
        ),
    )
    .await;
    let pseud = uuid::Uuid::new_v4().to_string();
    exec(
        db,
        &format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at)
             VALUES ('{pseud}', '{account}', 'qr-{}', 'QR', '{NOW}', '{NOW}')",
            &pseud[..8]
        ),
    )
    .await;
    (account, pseud)
}

/// A published work owned by `pseud`.
async fn work(db: &Database, pseud: &str) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    exec(
        db,
        &format!(
            "INSERT INTO works (id, owner_pseud_id, title, lifecycle, visibility, created_at, updated_at)
             VALUES ('{id}', '{pseud}', 'A Work', 'published', 'public', '{NOW}', '{NOW}')"
        ),
    )
    .await;
    id
}

#[tokio::test]
async fn a_reaction_is_one_row_per_reader_and_is_idempotent() {
    let db = connect("once").await;
    let (_, alice) = reader(&db).await;
    let (_, author) = reader(&db).await;
    let work = work(&db, &author).await;

    quick_reactions::add(&db, &work, &alice, "made_me_cry", NOW)
        .await
        .expect("react");
    quick_reactions::add(&db, &work, &alice, "made_me_cry", NOW)
        .await
        .expect("a double-tap is not an error");

    assert_eq!(
        quick_reactions::tally(&db, &work).await.expect("tally"),
        vec![("made_me_cry".to_owned(), 1)],
        "one reader, one reaction"
    );
}

#[tokio::test]
async fn one_reader_may_hold_several_labels_on_the_same_work() {
    let db = connect("multi").await;
    let (_, alice) = reader(&db).await;
    let (_, author) = reader(&db).await;
    let work = work(&db, &author).await;

    for label in ["made_me_cry", "worldbuilding", "comforting"] {
        quick_reactions::add(&db, &work, &alice, label, NOW)
            .await
            .expect("react");
    }

    // The order is the vocabulary's, not the table's.
    assert_eq!(
        quick_reactions::mine(&db, &work, &alice)
            .await
            .expect("mine"),
        vec![
            "made_me_cry".to_owned(),
            "comforting".to_owned(),
            "worldbuilding".to_owned()
        ]
    );
}

#[tokio::test]
async fn the_tally_counts_readers_and_orders_most_reacted_first() {
    let db = connect("tally").await;
    let (_, author) = reader(&db).await;
    let work = work(&db, &author).await;

    for _ in 0..3 {
        let (_, reader_pseud) = reader(&db).await;
        quick_reactions::add(&db, &work, &reader_pseud, "banter", NOW)
            .await
            .expect("react");
    }
    let (_, one) = reader(&db).await;
    quick_reactions::add(&db, &work, &one, "worldbuilding", NOW)
        .await
        .expect("react");

    let tally = quick_reactions::tally(&db, &work).await.expect("tally");
    assert_eq!(
        tally,
        vec![("banter".to_owned(), 3), ("worldbuilding".to_owned(), 1)]
    );
    // Two reads agree, so the ordering is a property of the query and not of row luck.
    assert_eq!(
        tally,
        quick_reactions::tally(&db, &work).await.expect("tally")
    );
}

#[tokio::test]
async fn withdrawing_is_silent_and_idempotent() {
    let db = connect("withdraw").await;
    let (_, alice) = reader(&db).await;
    let (_, author) = reader(&db).await;
    let work = work(&db, &author).await;

    quick_reactions::add(&db, &work, &alice, "this_scene", NOW)
        .await
        .expect("react");
    assert!(quick_reactions::remove(&db, &work, &alice, "this_scene")
        .await
        .expect("withdraw"));
    assert!(
        !quick_reactions::remove(&db, &work, &alice, "this_scene")
            .await
            .expect("withdrawing twice is not an error"),
        "the second withdrawal reports that there was nothing to remove"
    );
    assert!(quick_reactions::tally(&db, &work)
        .await
        .expect("tally")
        .is_empty());
}

#[tokio::test]
async fn the_authors_feedback_preference_gates_the_bar() {
    let db = connect("gate").await;
    let (author_account, author) = reader(&db).await;
    let work = work(&db, &author).await;

    assert!(
        quick_reactions::enabled_for(&db, &work)
            .await
            .expect("enabled"),
        "a work with no preference row is enabled"
    );

    // The account default, with no work override.
    let b = boolean(&db, false);
    exec(
        &db,
        &format!(
            "INSERT INTO feedback_preferences
             (account_id, accept_constructive, ambiguous_auto_deliver, comments_enabled, author_note, created_at, updated_at, version)
             VALUES ('{author_account}', {b}, {b}, {b}, '', '{NOW}', '{NOW}', 1)"
        ),
    )
    .await;
    assert!(
        !quick_reactions::enabled_for(&db, &work)
            .await
            .expect("enabled"),
        "the owner's account default applies"
    );

    // A work override wins over the account default.
    let b = boolean(&db, true);
    exec(
        &db,
        &format!(
            "INSERT INTO work_feedback_preferences
             (work_id, accept_constructive, ambiguous_auto_deliver, comments_enabled, author_note, updated_at, version)
             VALUES ('{work}', NULL, NULL, {b}, NULL, '{NOW}', 1)"
        ),
    )
    .await;
    assert!(
        quick_reactions::enabled_for(&db, &work)
            .await
            .expect("enabled"),
        "the work override is checked before the account default"
    );
}

#[tokio::test]
async fn an_unknown_work_is_not_enabled() {
    let db = connect("missing").await;
    assert!(
        !quick_reactions::enabled_for(&db, &uuid::Uuid::new_v4().to_string())
            .await
            .expect("enabled")
    );
}
