//! M64-11 — following a shelf.
//!
//! A subscription to a list is only meaningful if it cannot become a subscription to something nobody published. This test
//! pins the three refusals and the one behaviour that makes the feature safe over time: a shelf made **private after** it was
//! followed keeps the row but disappears from the follower's read.
//!
//! Asserts:
//! 1. following a public shelf works and is idempotent (two calls, one row);
//! 2. following your own shelf is refused as `OwnShelf`;
//! 3. following a private shelf is refused as `NotPublic`;
//! 4. following a shelf that does not exist is `NotFound`;
//! 5. after the followed shelf is made private, `followed_shelves` no longer lists it while the row survives — the follower's
//!    subscription is not silently deleted by someone else's decision;
//! 6. unfollowing removes the row and reports whether there was one.

use darkhold_db::{Database, DatabaseConfig};
use std::path::PathBuf;

const ME: &str = "01955100-0000-4000-8000-000000000001";
const OTHER: &str = "01955100-0000-4000-8000-000000000002";
const PUBLIC_SHELF: &str = "01955100-0000-4000-8000-000000000003";
const PRIVATE_SHELF: &str = "01955100-0000-4000-8000-000000000004";
const MY_SHELF: &str = "01955100-0000-4000-8000-000000000005";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-follows-{tag}-{}-{:?}",
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
                "lh_test_follows_{}_{}",
                std::process::id(),
                COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            );
            test_support::empty_postgres(&admin_url, &name)
                .await
                .expect("an empty PostgreSQL database")
        }
        _ => {
            let config = DatabaseConfig::new(format!(
                "sqlite://{}/follows.sqlite?mode=rwc",
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
    exec(
        db,
        &format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at,
                                 updated_at, version)
             VALUES ('01955100-0000-4000-8000-000000000006','{OTHER}','shelflord','Shelflord','listed',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;
    for (id, owner, name, public) in [
        (PUBLIC_SHELF, OTHER, "Published list", 1),
        (PRIVATE_SHELF, OTHER, "Private list", 0),
        (MY_SHELF, ME, "Mine", 1),
    ] {
        exec(
            db,
            &format!(
                "INSERT INTO shelves (id, account_id, name, is_public, created_at, updated_at, version)
                 VALUES ('{id}','{owner}','{name}',{public},
                         '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
            ),
        )
        .await;
    }
}

#[tokio::test]
async fn a_follow_is_public_only_and_survives_the_shelf_going_private() {
    use darkhold_db::shelf_follows::{follow, followed_shelves, follows, unfollow, FollowRefusal};

    let (db, dir) = database("rules").await;
    seed(&db).await;

    // 1. Following works, and twice is once.
    assert_eq!(follow(&db, ME, PUBLIC_SHELF).await.expect("follow"), Ok(()));
    assert_eq!(
        follow(&db, ME, PUBLIC_SHELF).await.expect("follow again"),
        Ok(()),
        "a client retrying a POST is not a second reader"
    );
    assert!(follows(&db, ME, PUBLIC_SHELF).await.expect("follows"));

    // 2-4. The three refusals, each distinct because each has a different fix.
    assert_eq!(
        follow(&db, ME, MY_SHELF).await.expect("own shelf"),
        Err(FollowRefusal::OwnShelf)
    );
    assert_eq!(
        follow(&db, ME, PRIVATE_SHELF).await.expect("private"),
        Err(FollowRefusal::NotPublic)
    );
    assert_eq!(
        follow(&db, ME, "01955100-0000-4000-8000-0000000000ff")
            .await
            .expect("absent"),
        Err(FollowRefusal::NotFound)
    );

    // 5. The shelf goes private: the read stops listing it, the row survives.
    let listed = followed_shelves(&db, ME).await.expect("list");
    assert_eq!(listed.len(), 1, "one followed shelf: {listed:?}");
    assert_eq!(listed[0].shelf_id, PUBLIC_SHELF);
    assert_eq!(listed[0].owner_handle.as_deref(), Some("shelflord"));

    exec(
        &db,
        &format!("UPDATE shelves SET is_public = 0 WHERE id = '{PUBLIC_SHELF}'"),
    )
    .await;
    let after = followed_shelves(&db, ME).await.expect("list after");
    assert!(
        after.is_empty(),
        "a shelf made private is not shown to its followers: {after:?}"
    );
    assert!(
        follows(&db, ME, PUBLIC_SHELF).await.expect("still a row"),
        "the follow row survives: unfollowing people is not the owner's decision to make"
    );

    // 6. Unfollowing reports whether there was anything to remove.
    assert!(unfollow(&db, ME, PUBLIC_SHELF).await.expect("unfollow"));
    assert!(
        !unfollow(&db, ME, PUBLIC_SHELF)
            .await
            .expect("unfollow twice"),
        "the second unfollow removed nothing and says so"
    );

    let _ = std::fs::remove_dir_all(dir);
}
