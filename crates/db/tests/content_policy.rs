//! M69-16 layer two — the instance's content policy.
//!
//! Two properties are worth more than the CRUD, and they are the reason this test exists:
//!
//! 1. **a policy block reaches a reader with no session.** `for_pseud(db, None)` must still return the instance's blocks, because a
//!    signed-out visitor is a reader too, and a filter that only applies once you log in is a filter that leaks to everyone
//!    before they do;
//! 2. **the reason is required and the revocation is a record, not a delete.** A block with no reason cannot be defended or
//!    revoked with confidence, and a trail that disappears when the block is lifted cannot answer "why was this hidden in March".

use darkhold_db::{Database, DatabaseConfig};
use std::path::PathBuf;

const ADMIN: &str = "01955300-0000-4000-8000-000000000001";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-policy-{tag}-{}-{:?}",
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
                "lh_test_policy_{}_{}",
                std::process::id(),
                COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            );
            test_support::empty_postgres(&admin_url, &name)
                .await
                .expect("an empty PostgreSQL database")
        }
        _ => {
            let config =
                DatabaseConfig::new(format!("sqlite://{}/policy.sqlite?mode=rwc", dir.display()));
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

#[tokio::test]
async fn a_policy_block_needs_a_reason_reaches_the_signed_out_and_keeps_a_trail() {
    use darkhold_db::content_policy::{block, history, live, revoke, BlockRefusal};

    let (db, dir) = database("layer-two").await;
    exec(
        &db,
        &format!(
            "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version)
             VALUES ('{ADMIN}','active','admin@example.invalid','adult',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;

    // A block with no reason is refused, and nothing is written.
    assert_eq!(
        block(&db, "tag", "child endangerment", "   ", ADMIN)
            .await
            .expect("block"),
        Err(BlockRefusal::EmptyReason)
    );
    assert!(live(&db).await.expect("live").is_empty());

    assert_eq!(
        block(
            &db,
            "tag",
            "child endangerment",
            "illegal content, §46.2",
            ADMIN
        )
        .await
        .expect("block"),
        Ok(())
    );
    // Blocking the same value twice is refused rather than duplicated.
    assert_eq!(
        block(&db, "tag", "child endangerment", "again", ADMIN)
            .await
            .expect("block"),
        Err(BlockRefusal::AlreadyBlocked)
    );

    assert_eq!(live(&db).await.expect("live").len(), 1);

    // The property that matters: a reader with no session still gets the rule.
    let signed_out = darkhold_db::search::content_filter_sql::for_pseud(&db, None)
        .await
        .expect("filter rules for a signed-out reader");
    assert!(
        signed_out
            .iter()
            .any(|rule| rule.filter_type == "tag" && rule.value == "child endangerment"),
        "the instance's policy must apply before sign-in: {signed_out:?}"
    );

    // Revocation needs a reason too, and the row survives it.
    assert_eq!(
        revoke(&db, "tag", "child endangerment", "", ADMIN)
            .await
            .expect("revoke"),
        Err(BlockRefusal::EmptyReason)
    );
    assert_eq!(
        revoke(
            &db,
            "tag",
            "child endangerment",
            "court order withdrawn",
            ADMIN
        )
        .await
        .expect("revoke"),
        Ok(true)
    );
    assert!(live(&db).await.expect("live").is_empty());
    assert!(
        darkhold_db::search::content_filter_sql::for_pseud(&db, None)
            .await
            .expect("rules")
            .is_empty()
    );

    let trail = history(&db).await.expect("history");
    assert_eq!(trail.len(), 1, "the block is not deleted: {trail:?}");
    assert!(trail[0].3.is_some(), "revocation time recorded");
    assert_eq!(trail[0].4.as_deref(), Some("court order withdrawn"));

    // And the value can be blocked again afterwards, because the unique index is partial.
    assert_eq!(
        block(&db, "tag", "child endangerment", "reinstated", ADMIN)
            .await
            .expect("block again"),
        Ok(())
    );
    assert_eq!(history(&db).await.expect("history").len(), 2);

    let _ = std::fs::remove_dir_all(dir);
}
