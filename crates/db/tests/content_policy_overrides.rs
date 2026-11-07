//! M69-16's last clause — a recorded exception to an instance policy block.
//!
//! The exclusion is rendered once and spliced into every surface, so the only honest way to test the override is to render the
//! real predicate and run it: a test that inspected the SQL string would pass on a clause in the wrong scope.
//!
//! Asserts:
//! 1. a policy block hides every work carrying the value;
//! 2. an override, with a reason, makes **one** work visible and leaves the other hidden;
//! 3. revoking the override hides it again, and the row (with its reason) survives;
//! 4. an override of a rule nobody blocked is refused — it would be a decision about nothing;
//! 5. an override with a blank reason is refused.

use darkhold_db::search::content_filter_sql::exclusion_for;
use darkhold_db::{Database, DatabaseConfig};
use std::path::PathBuf;

const ADMIN: &str = "01955400-0000-4000-8000-000000000001";
const AUTHOR: &str = "01955400-0000-4000-8000-000000000002";
const PSEUD: &str = "01955400-0000-4000-8000-000000000003";
const NODE: &str = "01955400-0000-4000-8000-000000000004";
const WORK_A: &str = "01955400-0000-4000-8000-000000000005";
const WORK_B: &str = "01955400-0000-4000-8000-000000000006";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-override-{tag}-{}-{:?}",
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
                "lh_test_override_{}_{}",
                std::process::id(),
                COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            );
            test_support::empty_postgres(&admin_url, &name)
                .await
                .expect("an empty PostgreSQL database")
        }
        _ => {
            let config = DatabaseConfig::new(format!(
                "sqlite://{}/override.sqlite?mode=rwc",
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

/// The works a filtered query returns, using the **real** rendered exclusion.
async fn visible(
    db: &Database,
    rules: &[darkhold_db::search::content_filter_sql::FilterRule],
) -> Vec<String> {
    let exclusion = exclusion_for(rules, "w.id");
    // Splice exactly as the real callers do (`notifications.rs`): an empty predicate adds nothing, a present one is prefixed
    // with `AND`. Getting this wrong is a syntax error, which is the good failure; the bad one would be a `WHERE 1 = 1` that
    // silently dropped the filter.
    let and = if exclusion.predicate.is_empty() {
        String::new()
    } else {
        format!(" AND {}", exclusion.predicate)
    };
    let sqlite = format!("SELECT CAST(w.id AS TEXT) FROM works w WHERE 1 = 1{and}");
    let postgres = format!("SELECT w.id::text FROM works w WHERE 1 = 1{and}");
    let sql = db.sql(&sqlite, &postgres);
    let mut ids: Vec<String> = match db.backend() {
        darkhold_db::Backend::Sqlite => {
            let mut query = sqlx::query_scalar::<_, String>(&sql);
            for bind in &exclusion.binds {
                query = query.bind(bind.as_str());
            }
            query
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("query")
        }
        darkhold_db::Backend::Postgres => {
            let mut query = sqlx::query_scalar::<_, String>(&sql);
            for bind in &exclusion.binds {
                query = query.bind(bind.as_str());
            }
            query
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await
                .expect("query")
        }
    };
    ids.sort();
    ids
}

async fn seed(db: &Database) {
    for (account, email) in [
        (ADMIN, "admin@example.invalid"),
        (AUTHOR, "author@example.invalid"),
    ] {
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
             VALUES ('{PSEUD}','{AUTHOR}','authora','Author A','listed',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at)
             VALUES ('{NODE}','tag','banned trope','banned trope','2026-01-01T00:00:00Z')"
        ),
    )
    .await;
    for (work, title) in [(WORK_A, "Archive copy"), (WORK_B, "Ordinary fic")] {
        exec(
            db,
            &format!(
                "INSERT INTO works (id, owner_pseud_id, title, lifecycle, visibility, completion,
                                    created_at, updated_at, version)
                 VALUES ('{work}','{PSEUD}','{title}','published','public','complete',
                         '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
            ),
        )
        .await;
        exec(
            db,
            &format!(
                "INSERT INTO work_tags (work_id, node_id, prominence, added_at)
                 VALUES ('{work}','{NODE}',1,'2026-01-01T00:00:00Z')"
            ),
        )
        .await;
    }
}

#[tokio::test]
async fn an_override_lets_one_work_through_one_block_and_leaves_a_record() {
    use darkhold_db::content_policy::{block, grant_override, live, revoke_override, BlockRefusal};
    use darkhold_db::search::content_filter_sql::{for_pseud, FilterRule};

    let (db, dir) = database("one-work").await;
    seed(&db).await;

    // Both works are visible with no rules.
    assert_eq!(
        visible(&db, &[]).await,
        vec![WORK_A.to_string(), WORK_B.to_string()]
    );

    // 1. The block hides both.
    assert_eq!(
        block(&db, "tag", "banned trope", "§46.2", ADMIN)
            .await
            .expect("block"),
        Ok(())
    );
    let rules = for_pseud(&db, None).await.expect("rules");
    assert!(
        visible(&db, &rules).await.is_empty(),
        "both works carry the value"
    );

    // 4-5. Refusals first: no live block, no reason.
    assert_eq!(
        grant_override(
            &db,
            WORK_A,
            "tag",
            "a trope nobody blocked",
            "why not",
            ADMIN
        )
        .await
        .expect("grant"),
        Err(BlockRefusal::NotFound),
        "an exception to nothing is a decision about nothing"
    );
    assert_eq!(
        grant_override(&db, WORK_A, "tag", "banned trope", "  ", ADMIN)
            .await
            .expect("grant"),
        Err(BlockRefusal::EmptyReason)
    );

    // 2. One override, one work.
    assert_eq!(
        grant_override(
            &db,
            WORK_A,
            "tag",
            "banned trope",
            "scholarly archive of the trope, reviewed 2026-11",
            ADMIN
        )
        .await
        .expect("grant"),
        Ok(())
    );
    assert_eq!(
        live(&db).await.expect("live").len(),
        1,
        "the block itself is untouched"
    );
    let rules = for_pseud(&db, None).await.expect("rules");
    assert_eq!(
        visible(&db, &rules).await,
        vec![WORK_A.to_string()],
        "the exception covers the work it names and no other"
    );

    // 3. Revoking restores the exclusion; the trail keeps the reason.
    assert_eq!(
        revoke_override(&db, WORK_A, "tag", "banned trope", "archive closed", ADMIN)
            .await
            .expect("revoke"),
        Ok(true)
    );
    let rules = for_pseud(&db, None).await.expect("rules");
    assert!(
        visible(&db, &rules).await.is_empty(),
        "a revoked exception hides the work again"
    );
    let rows: i64 = match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar(
            "SELECT COUNT(*) FROM content_policy_overrides WHERE revoked_reason = 'archive closed'",
        )
        .fetch_one(db.sqlite_pool().expect("sqlite"))
        .await
        .expect("count"),
        darkhold_db::Backend::Postgres => sqlx::query_scalar(
            "SELECT COUNT(*) FROM content_policy_overrides WHERE revoked_reason = 'archive closed'",
        )
        .fetch_one(db.postgres_pool().expect("postgres"))
        .await
        .expect("count"),
    };
    assert_eq!(rows, 1, "the exception is revoked, not deleted");

    // A rule the reader owns is unaffected: the override exempts one *policy* rule, and the pairwise rendering keeps that true.
    let own = vec![FilterRule::new("tag", "banned trope")];
    assert!(visible(&db, &own).await.is_empty());

    let _ = std::fs::remove_dir_all(dir);
}
