//! M69-21 — a saved view has a name its reader chose.
//!
//! The address is the smallest part of a saved filter that has to be right: a UUID is fine to store and useless to type, share or
//! put on a key. This test pins the three properties the slug column relies on — uniqueness **per account**, validity by refusal
//! rather than by normalisation, and resolution that cannot cross accounts.
//!
//! Asserts:
//! 1. a legal name round-trips through `saved_view_id_for_slug`;
//! 2. an illegal name (`To Read`, `-lead`, `a--b`, 65 chars) is refused, and the view keeps the name it had;
//! 3. two views of one account cannot share a name;
//! 4. two accounts may each have `to-read`, and one account's slug never resolves in the other's scope;
//! 5. clearing a name is possible and leaves the view addressable by id.

use darkhold_db::{Database, DatabaseConfig};
use std::path::PathBuf;

const ME: &str = "01955200-0000-4000-8000-000000000001";
const OTHER: &str = "01955200-0000-4000-8000-000000000002";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-slug-{tag}-{}-{:?}",
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
                "lh_test_slug_{}_{}",
                std::process::id(),
                COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            );
            test_support::empty_postgres(&admin_url, &name)
                .await
                .expect("an empty PostgreSQL database")
        }
        _ => {
            let config =
                DatabaseConfig::new(format!("sqlite://{}/slug.sqlite?mode=rwc", dir.display()));
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
}

async fn view(db: &Database, account: &str, id: &str, name: &str) {
    exec(
        db,
        &format!(
            "INSERT INTO saved_views (id, account_id, name, query_json, query_version, sort, scope,
                                      pinned, created_at, updated_at, version)
             VALUES ('{id}','{account}','{name}','{{\"tag\":\"x\"}}',1,'recent','library',0,
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;
}

#[tokio::test]
async fn a_saved_view_can_be_named_and_the_name_is_per_account() {
    use darkhold_db::library::{saved_view_id_for_slug, set_saved_view_slug, valid_slug};

    let (db, dir) = database("names").await;
    seed(&db).await;
    view(&db, ME, "01955200-0000-4000-8000-00000000000a", "Morning").await;
    view(&db, ME, "01955200-0000-4000-8000-00000000000b", "Evening").await;

    // 1. A legal name round-trips.
    assert!(set_saved_view_slug(
        &db,
        ME,
        "01955200-0000-4000-8000-00000000000a",
        Some("to-read")
    )
    .await
    .expect("name it"));
    assert_eq!(
        saved_view_id_for_slug(&db, ME, "to-read")
            .await
            .expect("resolve"),
        Some("01955200-0000-4000-8000-00000000000a".to_owned())
    );

    // 2. Illegal names are refused, and nothing is written.
    for bad in ["To Read", "-lead", "trail-", "a--b", &"x".repeat(65)] {
        assert!(!valid_slug(bad), "`{bad}` should be refused");
        assert!(
            set_saved_view_slug(&db, ME, "01955200-0000-4000-8000-00000000000b", Some(bad))
                .await
                .is_err(),
            "`{bad}` must not be accepted"
        );
    }
    assert_eq!(
        saved_view_id_for_slug(&db, ME, "to-read")
            .await
            .expect("resolve"),
        Some("01955200-0000-4000-8000-00000000000a".to_owned()),
        "a refused name leaves the other view's name intact"
    );

    // 3. Two of my views cannot share a name.
    assert!(
        set_saved_view_slug(
            &db,
            ME,
            "01955200-0000-4000-8000-00000000000b",
            Some("to-read")
        )
        .await
        .is_err(),
        "the account may hold `to-read` once"
    );

    // 4. Another account may use the same name, and never resolves mine.
    view(&db, OTHER, "01955200-0000-4000-8000-00000000000c", "Theirs").await;
    assert!(set_saved_view_slug(
        &db,
        OTHER,
        "01955200-0000-4000-8000-00000000000c",
        Some("to-read")
    )
    .await
    .expect("their own name"));
    assert_eq!(
        saved_view_id_for_slug(&db, OTHER, "to-read")
            .await
            .expect("resolve"),
        Some("01955200-0000-4000-8000-00000000000c".to_owned()),
        "the same slug in another account is a different view"
    );

    // 5. Clearing the name leaves the view addressable by id only.
    assert!(
        set_saved_view_slug(&db, ME, "01955200-0000-4000-8000-00000000000a", None)
            .await
            .expect("clear")
    );
    assert_eq!(
        saved_view_id_for_slug(&db, ME, "to-read")
            .await
            .expect("resolve"),
        None
    );

    let _ = std::fs::remove_dir_all(dir);
}
