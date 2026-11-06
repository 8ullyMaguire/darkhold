//! M64-11 — co-shelving: what readers who shelved your work also shelved.
//!
//! The failure this guards is a recommendation built on a shelf nobody published. A private shelf is a reading list its owner
//! kept to themselves, and `co_shelving` reads other readers' shelves — so `is_public = 1` is a correctness condition, not a
//! nicety, and this test fails if it is dropped.
//!
//! Asserts:
//! 1. a work on a **public** shelf alongside something the reader shelved is recommended, once per shared shelf;
//! 2. a work on a **private** shelf alongside the same work is **not** recommended — the reader never sees another person's
//!    private collection;
//! 3. works the reader has shelved themselves are not recommended back to them;
//! 4. an unpublished or non-public target is not recommended, because inferring from an unreachable work is a claim about
//!    readers made from a non-event.

use darkhold_db::{Database, DatabaseConfig};
use std::path::PathBuf;

const ME: &str = "01955000-0000-4000-8000-000000007001";
const OTHER: &str = "01955000-0000-4000-8000-000000007002";
const PSEUD_ME: &str = "01955000-0000-4000-8000-000000007003";
const PSEED: &str = "01955000-0000-4000-8000-000000007004";

const MINE: &str = "01955000-0000-4000-8000-000000007005"; // I shelved this
const PUBLIC_PEER: &str = "01955000-0000-4000-8000-000000007006"; // on a public shelf with MINE
const PRIVATE_PEER: &str = "01955000-0000-4000-8000-000000007007"; // on a private shelf with MINE
const DRAFT_PEER: &str = "01955000-0000-4000-8000-000000007008"; // public shelf, but not published

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-coshelf-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

async fn database(tag: &str) -> (Database, PathBuf) {
    let dir = scratch_dir(tag);
    let db = match std::env::var("DARKHOLD_TEST_PG_URL") {
        Ok(admin_url) if !admin_url.is_empty() => {
            static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let name = format!(
                "lh_test_coshelf_{}_{}",
                std::process::id(),
                COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            );
            test_support::empty_postgres(&admin_url, &name)
                .await
                .expect("an empty PostgreSQL database")
        }
        _ => {
            let config = DatabaseConfig::new(format!(
                "sqlite://{}/coshelf.sqlite?mode=rwc",
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

async fn seed(db: &Database) {
    for (account, email) in [(ME, "me@example.invalid"), (OTHER, "other@example.invalid")] {
        exec(
            db,
            &format!(
                "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version)
                 VALUES ('{account}','active','{email}','adult',
                         '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
            ),
        )
        .await;
    }
    for (pseud, account, handle) in [(PSEUD_ME, ME, "meshelf"), (PSEED, OTHER, "othershelf")] {
        exec(
            db,
            &format!(
                "INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at,
                                     updated_at, version)
                 VALUES ('{pseud}','{account}','{handle}','{handle}','listed',
                         '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
            ),
        )
        .await;
    }
    for (id, owner, title, lifecycle) in [
        (MINE, PSEUD_ME, "Mine", "published"),
        (PUBLIC_PEER, PSEED, "Public peer", "published"),
        (PRIVATE_PEER, PSEED, "Private peer", "published"),
        (DRAFT_PEER, PSEED, "Draft peer", "draft"),
    ] {
        exec(
            db,
            &format!(
                "INSERT INTO works (id, owner_pseud_id, title, language, rating, visibility, lifecycle,
                                    created_at, updated_at, version)
                 VALUES ('{id}','{owner}','{title}','en','general','public','{lifecycle}',
                         '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
            ),
        )
        .await;
    }

    // My shelf, private by construction, holding the work I shelved.
    let mine_shelf = "01955000-0000-4000-8000-000000007009";
    let mine_item = "01955000-0000-4000-8000-00000000700a";
    exec(
        db,
        &format!(
            "INSERT INTO shelves (id, account_id, name, created_at, updated_at, version)
             VALUES ('{mine_shelf}','{ME}','Mine','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO library_items (id, account_id, work_id, source_key, source_work_key, title,
                                        source_url, created_at, updated_at, version)
             VALUES ('{mine_item}','{ME}','{MINE}','manual','{MINE}','Mine',
                     'https://example.invalid/m','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO shelf_items (id, shelf_id, library_item_id, created_at)
             VALUES ('01955000-0000-4000-8000-00000000700b','{mine_shelf}','{mine_item}',
                     '2026-01-01T00:00:00Z')"
        ),
    )
    .await;

    // Two other-reader shelves: one public, one private. Both hold MINE; the public one holds PUBLIC_PEER and DRAFT_PEER, the
    // private one holds PRIVATE_PEER.
    for (shelf, name, public, item, work, id) in [
        (
            "01955000-0000-4000-8000-00000000700c",
            "Public list",
            1,
            "01955000-0000-4000-8000-00000000700d",
            MINE,
            "01955000-0000-4000-8000-00000000700e",
        ),
        (
            "01955000-0000-4000-8000-00000000700c",
            "Public list",
            1,
            "01955000-0000-4000-8000-00000000700f",
            PUBLIC_PEER,
            "01955000-0000-4000-8000-000000007010",
        ),
        (
            "01955000-0000-4000-8000-00000000700c",
            "Public list",
            1,
            "01955000-0000-4000-8000-000000007011",
            DRAFT_PEER,
            "01955000-0000-4000-8000-000000007012",
        ),
        (
            "01955000-0000-4000-8000-000000007013",
            "Private list",
            0,
            "01955000-0000-4000-8000-000000007014",
            MINE,
            "01955000-0000-4000-8000-000000007015",
        ),
        (
            "01955000-0000-4000-8000-000000007013",
            "Private list",
            0,
            "01955000-0000-4000-8000-000000007016",
            PRIVATE_PEER,
            "01955000-0000-4000-8000-000000007017",
        ),
    ] {
        exec(
            db,
            &format!(
                "INSERT INTO shelves (id, account_id, name, is_public, created_at, updated_at, version)
                 VALUES ('{shelf}','{OTHER}','{name}',{public},
                         '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)
                 ON CONFLICT (id) DO NOTHING"
            ),
        )
        .await;
        exec(
            db,
            &format!(
                "INSERT INTO library_items (id, account_id, work_id, source_key, source_work_key, title,
                                            source_url, created_at, updated_at, version)
                 VALUES ('{item}','{OTHER}','{work}','manual','{item}','x',
                         'https://example.invalid/x','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
            ),
        )
        .await;
        exec(
            db,
            &format!(
                "INSERT INTO shelf_items (id, shelf_id, library_item_id, created_at)
                 VALUES ('{id}','{shelf}','{item}','2026-01-01T00:00:00Z')"
            ),
        )
        .await;
    }
}

#[tokio::test]
async fn co_shelving_reads_public_shelves_and_never_a_private_one() {
    let (db, dir) = database("public").await;
    seed(&db).await;

    let recommended = darkhold_db::discovery::co_shelving_recommendations(&db, ME, 20)
        .await
        .expect("read co-shelving");
    let ids: Vec<String> = recommended.iter().map(|id| id.to_string()).collect();

    assert_eq!(
        ids.len(),
        1,
        "exactly the published work on the public shelf: {ids:?}"
    );
    assert!(
        ids[0].eq_ignore_ascii_case(PUBLIC_PEER),
        "the public peer is recommended: {ids:?}"
    );
    assert!(
        !ids.iter().any(|id| id.eq_ignore_ascii_case(PRIVATE_PEER)),
        "a private shelf is not a source: {ids:?}"
    );
    assert!(
        !ids.iter().any(|id| id.eq_ignore_ascii_case(DRAFT_PEER)),
        "an unpublished target is not a recommendation: {ids:?}"
    );
    assert!(
        !ids.iter().any(|id| id.eq_ignore_ascii_case(MINE)),
        "the work the reader already shelved is not recommended back: {ids:?}"
    );

    let _ = std::fs::remove_dir_all(dir);
}
