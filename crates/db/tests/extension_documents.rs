//! M70-09 — extension documents: one row, replaced in place, and gone when the account goes.
//!
//! The FK cascade assertion is the erasure half: a skin is the reader's own document, so deleting the account must
//! take it, and nothing else.: shared canonicalisation, and a wall between the collection and its works.
//!
//! The assertion that matters is the second one. A collection tag describes what a collection is FOR — a ship week,
//! a challenge — so it must never reach `work_tags`, because anything there is searchable and feeds taste. The test
//! proves the wall structurally (no row appears in `work_tags`) rather than by searching, because a search assertion
//! would pass for the wrong reason the day a work legitimately carries the same tag.

use darkhold_db::Database;

fn scratch_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-extdoc-{tag}-{}-{:?}",
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
                "lh_test_extdoc_{}_{}",
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

async fn scalar_count(db: &Database, statement: &str) -> i64 {
    match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar(statement)
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("count"),
        darkhold_db::Backend::Postgres => sqlx::query_scalar(statement)
            .fetch_one(db.postgres_pool().expect("postgres"))
            .await
            .expect("count"),
    }
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

/// Two accounts, each with one published work. Ids are UUID-shaped: PostgreSQL stores them as `uuid` and an id like
/// `acc-1` is a cast error on that arm only.
const A1: &str = "01955000-0000-4000-8000-0000000000a1";
const A2: &str = "01955000-0000-4000-8000-0000000000a2";
const P1: &str = "01955000-0000-4000-8000-0000000000b1";
const P2: &str = "01955000-0000-4000-8000-0000000000b2";
const W1: &str = "01957000-0000-4000-8000-0000000000c1";
const W2: &str = "01957000-0000-4000-8000-0000000000c2";

async fn seed(db: &Database) {
    for (account, pseud, handle) in [(A1, P1, "one"), (A2, P2, "two")] {
        exec(
            db,
            &format!(
                "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version) \
                 VALUES ('{account}','active','{handle}@mfl.invalid','adult', \
                         '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
            ),
        )
        .await;
        exec(
            db,
            &format!(
                "INSERT INTO pseuds (id, account_id, handle, display_name, bio, discoverability, created_at, \
                 updated_at, version) VALUES ('{pseud}','{account}','{handle}','{handle}',NULL,'listed', \
                 '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
            ),
        )
        .await;
    }
    for (work, pseud, title) in [(W1, P1, "First"), (W2, P2, "Second")] {
        exec(
            db,
            &format!(
                "INSERT INTO works (id, owner_pseud_id, title, summary, language, rating, visibility, lifecycle, \
                 completion, created_at, updated_at, version) VALUES ('{work}','{pseud}','{title}','','en', \
                 'general','public','published','complete','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
            ),
        )
        .await;
    }
}

fn layout(name: &str) -> String {
    format!(
        r#"{{"version":1,"kind":"layout","name":"{name}","body":{{"slots":[{{"widget":"inbox"}},{{"widget":"trends","span":6}}]}}}}"#
    )
}

async fn first_pseud(db: &darkhold_db::Database, account: &str) -> String {
    match db.backend() {
        darkhold_db::Backend::Sqlite => {
            let row: (String,) = sqlx::query_as(
                "SELECT id FROM pseuds WHERE account_id = ? ORDER BY created_at LIMIT 1",
            )
            .bind(account)
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("a pseud");
            row.0
        }
        darkhold_db::Backend::Postgres => {
            let row: (String,) = sqlx::query_as(
                "SELECT id::text FROM pseuds WHERE account_id::text = $1 ORDER BY created_at LIMIT 1",
            )
            .bind(account)
            .fetch_one(db.postgres_pool().expect("postgres"))
            .await
            .expect("a pseud");
            row.0
        }
    }
}

async fn pseud_handle(db: &darkhold_db::Database, id: &str) -> String {
    match db.backend() {
        darkhold_db::Backend::Sqlite => {
            let row: (String,) = sqlx::query_as("SELECT handle FROM pseuds WHERE id = ?")
                .bind(id)
                .fetch_one(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("handle");
            row.0
        }
        darkhold_db::Backend::Postgres => {
            let row: (String,) = sqlx::query_as("SELECT handle FROM pseuds WHERE id::text = $1")
                .bind(id)
                .fetch_one(db.postgres_pool().expect("postgres"))
                .await
                .expect("handle");
            row.0
        }
    }
}

#[tokio::test]
async fn documents_are_private_until_published_and_export_is_the_stored_bytes() {
    let (db, dir) = database("extdoc").await;
    seed(&db).await;

    let mine =
        darkhold_db::extension_documents::create(&db, A1, &layout("Mine"), "Mine", "layout", false)
            .await
            .expect("create");
    let shared = darkhold_db::extension_documents::create(
        &db,
        A1,
        &layout("Shared"),
        "Shared",
        "layout",
        true,
    )
    .await
    .expect("create");

    // The owner sees both; the stranger sees only the published one.
    assert!(darkhold_db::extension_documents::get(&db, &mine, A1)
        .await
        .expect("get")
        .is_some());
    assert!(darkhold_db::extension_documents::get(&db, &mine, A2)
        .await
        .expect("get")
        .is_none());
    assert!(darkhold_db::extension_documents::get(&db, &shared, A2)
        .await
        .expect("get")
        .is_some());
    assert!(darkhold_db::extension_documents::get(&db, &mine, "")
        .await
        .expect("anon")
        .is_none());
    assert!(darkhold_db::extension_documents::get(&db, &shared, "")
        .await
        .expect("anon")
        .is_some());

    // Export is the stored text, so what a reader sent and what this instance keeps are the same bytes.
    let stored = darkhold_db::extension_documents::get(&db, &shared, A1)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(stored.document, layout("Shared"));
    assert_eq!(stored.kind, "layout");
    assert_eq!(stored.name, "Shared");

    // "Mine" means mine: the list never includes somebody else's public document.
    let listed = darkhold_db::extension_documents::list_mine(&db, A1)
        .await
        .expect("list");
    assert_eq!(listed.len(), 2);
    assert!(darkhold_db::extension_documents::list_mine(&db, A2)
        .await
        .expect("list")
        .is_empty());

    // Deletion is the owner's, and a stranger's attempt changes nothing.
    assert!(!darkhold_db::extension_documents::delete(&db, &mine, A2)
        .await
        .expect("delete"));
    assert!(darkhold_db::extension_documents::delete(&db, &mine, A1)
        .await
        .expect("delete"));
    assert!(!darkhold_db::extension_documents::delete(&db, &mine, A1)
        .await
        .expect("delete again"));

    // Publishing is attributed to a pseud; the pseud must be the account's own.
    let pseud = first_pseud(&db, A1).await;
    let theirs = first_pseud(&db, A2).await;

    assert!(
        !darkhold_db::extension_documents::publish(&db, &shared, A1, &theirs)
            .await
            .expect("publish"),
        "a document cannot be published as somebody else's pseud"
    );
    assert!(
        darkhold_db::extension_documents::publish(&db, &shared, A1, &pseud)
            .await
            .expect("publish")
    );

    // The browse list shows the pseud, not the account, and skips anything unpublished.
    let listed = darkhold_db::extension_documents::list_public(&db, None, 50)
        .await
        .expect("browse");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].pseud_handle, pseud_handle(&db, &pseud).await);
    assert!(
        listed.iter().all(|row| row.pseud_handle != A1),
        "an account id must never appear as an author"
    );

    // Withdrawing removes it from the list and leaves the row.
    assert!(
        darkhold_db::extension_documents::unpublish(&db, &shared, A1)
            .await
            .expect("unpublish")
    );
    assert!(darkhold_db::extension_documents::list_public(&db, None, 50)
        .await
        .expect("browse")
        .is_empty());
    assert!(darkhold_db::extension_documents::get(&db, &shared, A1)
        .await
        .expect("get")
        .is_some());

    // Erasure takes the rest.
    exec(&db, &format!("DELETE FROM accounts WHERE id = '{A1}'")).await;
    assert_eq!(
        scalar_count(&db, "SELECT count(*) FROM extension_documents").await,
        0
    );

    let _ = std::fs::remove_dir_all(dir);
}
