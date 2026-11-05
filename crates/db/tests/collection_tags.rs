//! M70-05 — collection tags: shared canonicalisation, and a wall between the collection and its works.
//!
//! The assertion that matters is the second one. A collection tag describes what a collection is FOR — a ship week,
//! a challenge — so it must never reach `work_tags`, because anything there is searchable and feeds taste. The test
//! proves the wall structurally (no row appears in `work_tags`) rather than by searching, because a search assertion
//! would pass for the wrong reason the day a work legitimately carries the same tag.

use darkhold_db::Database;

fn scratch_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-ctag-{tag}-{}-{:?}",
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
                "lh_test_ctag_{}_{}",
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

#[tokio::test]
async fn collection_tags_are_canonical_shared_and_never_reach_their_works() {
    let (db, dir) = database("tags").await;
    seed(&db).await;

    // A canonical taxonomy node, as the wrangling path would produce, and a collection owned by account one.
    exec(
        &db,
        "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at) \
         VALUES ('node-1','freeform','Yuletide 2026','yuletide 2026','2026-01-01T00:00:00Z')",
    )
    .await;
    let collection = "01958000-0000-4000-8000-000000000001";
    exec(
        &db,
        &format!(
            "INSERT INTO collections (id, name, description, owner, item_policy, is_public, created_at) \
             VALUES ('{collection}','Winter Exchange','','{A1}','open',1,'2026-01-01T00:00:00Z')"
        ),
    )
    .await;

    assert!(
        darkhold_db::collection_tags::add(&db, collection, "node-1")
            .await
            .expect("add"),
        "the first attach adds a row"
    );
    assert!(
        !darkhold_db::collection_tags::add(&db, collection, "node-1")
            .await
            .expect("add again"),
        "attaching twice is one tag, not an error"
    );

    let tags = darkhold_db::collection_tags::list(&db, collection)
        .await
        .expect("list");
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].canonical, "Yuletide 2026");
    assert_eq!(
        tags[0].kind, "freeform",
        "the kind travels with the tag so a client can group without a second call"
    );

    // **The wall.** Nothing was written where a work would see it, so the tag cannot surface in a work search or a
    // reader's taste vector.
    assert_eq!(
        scalar_count(
            &db,
            "SELECT count(*) FROM work_tags WHERE node_id = 'node-1'"
        )
        .await,
        0,
        "a collection tag must never become a work tag"
    );

    // Ownership is a fact about the collection, not about the tag.
    assert!(darkhold_db::collection_tags::owned_by(&db, collection, A1)
        .await
        .expect("owner"));
    assert!(!darkhold_db::collection_tags::owned_by(&db, collection, A2)
        .await
        .expect("other"));

    assert!(
        darkhold_db::collection_tags::remove(&db, collection, "node-1")
            .await
            .expect("remove"),
        "detaching removes the row"
    );
    assert!(
        !darkhold_db::collection_tags::remove(&db, collection, "node-1")
            .await
            .expect("remove again")
    );
    assert!(darkhold_db::collection_tags::list(&db, collection)
        .await
        .expect("list")
        .is_empty());

    let _ = std::fs::remove_dir_all(dir);
}
