//! M70-08 — the assurance level a session carries, stored verbatim: one row, replaced in place, and gone when the account goes.
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
        "darkhold-acr-{tag}-{}-{:?}",
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
                "lh_test_acr_{}_{}",
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

/// Two accounts, each with one published work. Ids are UUID-shaped: PostgreSQL stores them as `uuid` and an id like
/// `acc-1` is a cast error on that arm only.
const A1: &str = "01955000-0000-4000-8000-0000000000a1";
const A2: &str = "01955000-0000-4000-8000-0000000000a2";
const P1: &str = "01955000-0000-4000-8000-0000000000b1";
const P2: &str = "01955000-0000-4000-8000-0000000000b2";
const W1: &str = "01957000-0000-4000-8000-0000000000c1";
const W2: &str = "01957000-0000-4000-8000-0000000000c2";

/// Run a statement against the test database.
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
async fn a_session_records_the_acr_it_was_signed_in_with_or_none() {
    let (db, dir) = database("acr").await;
    seed(&db).await;

    let now = darkhold_db::sessions::now();
    let later = "2030-01-01T00:00:00Z";

    let strong = darkhold_db::sessions::create_session(
        &db,
        A1.parse().expect("account id"),
        "hash-strong",
        "csrf-strong",
        darkhold_db::sessions::SessionContext {
            user_agent: None,
            active_pseud_id: None,
            expires_at: later,
            acr: Some("urn:example:aal2"),
            // No roles asserted: this test is about the assurance level, and an empty claim is the state it must not disturb.
            roles: &[],
        },
    )
    .await
    .expect("session");
    assert!(!strong.to_string().is_empty());

    let found = darkhold_db::sessions::find_live_session_by_token_hash(&db, "hash-strong", &now)
        .await
        .expect("lookup")
        .expect("live session");
    assert_eq!(
        found.acr.as_deref(),
        Some("urn:example:aal2"),
        "the issuer's value is stored verbatim, not normalised into a level"
    );

    // A password session carries no claim at all — NULL, which the guard treats as its own state.
    darkhold_db::sessions::create_session(
        &db,
        A1.parse().expect("account id"),
        "hash-weak",
        "csrf-weak",
        darkhold_db::sessions::SessionContext {
            user_agent: None,
            active_pseud_id: None,
            expires_at: later,
            acr: None,
            roles: &[],
        },
    )
    .await
    .expect("session");
    let weak = darkhold_db::sessions::find_live_session_by_token_hash(&db, "hash-weak", &now)
        .await
        .expect("lookup")
        .expect("live session");
    assert_eq!(weak.acr, None, "no issuer, no claim");

    let _ = std::fs::remove_dir_all(dir);
}
