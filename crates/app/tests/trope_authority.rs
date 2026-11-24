//! Step 7: trope proposal authority from the real account and role tables.

use darkhold_app::config::TropesConfig;
use darkhold_app::trope_authority::{for_account, TropeRole};
use darkhold_db::{Backend, Database};
use std::path::PathBuf;

const NOW: &str = "2026-01-01T00:00:00Z";

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("darkhold-trope-authority-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

async fn exec(db: &Database, sql: &str) {
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(sql)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await
                .unwrap_or_else(|e| panic!("SQLite fixture: {e}"));
        }
        Backend::Postgres => {
            sqlx::query(sql)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await
                .unwrap_or_else(|e| panic!("PostgreSQL fixture: {e}"));
        }
    }
}

#[tokio::test]
async fn authority_roles_are_checked_before_trust() {
    let tdb = test_support::TestDb::connect_with_dir("trope-authority", &scratch()).await;
    let db = tdb.db();
    let cfg = TropesConfig {
        proposal_min_trust: 4,
        quorum: 2,
        voting_days: 7,
    };
    for (tail, level, curator, active_role) in [
        ("001", 2, false, false),
        ("002", 2, true, false),
        ("003", 4, false, false),
        ("004", 6, false, false),
        ("005", 4, false, true),
    ] {
        let id = format!("01957800-0000-4000-8000-00000000a{tail}");
        exec(
            db,
            &format!(
                "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version) \
                 VALUES ('{id}','active','{id}@test.dev','adult','{NOW}','{NOW}',1)"
            ),
        )
        .await;
        exec(
            db,
            &format!(
                "INSERT INTO trust_levels (account, level, computed_at, basis) \
                 VALUES ('{id}',{level},'{NOW}','test')"
            ),
        )
        .await;
        if curator {
            exec(
                db,
                &format!("UPDATE accounts SET is_curator = true WHERE id = '{id}'"),
            )
            .await;
        }
        if active_role {
            exec(
                db,
                &format!(
                    "INSERT INTO curator_roles (account_id, opted_in_by) VALUES ('{id}','test')"
                ),
            )
            .await;
        }
    }

    let account = |tail: &str| format!("01957800-0000-4000-8000-00000000a{tail}");
    let denied = for_account(db, &cfg, &account("001"))
        .await
        .expect("TL2 lookup");
    assert!(!denied.allowed, "{denied:?}");
    assert_eq!(denied.role, None);
    assert_eq!(denied.level, 2);

    let curator = for_account(db, &cfg, &account("002"))
        .await
        .expect("TL2 curator");
    assert!(curator.allowed, "{curator:?}");
    assert_eq!(
        curator.role,
        Some(TropeRole::Curator),
        "is_curator wins at TL2"
    );

    let trusted = for_account(db, &cfg, &account("003"))
        .await
        .expect("TL4 lookup");
    assert_eq!(trusted.role, Some(TropeRole::Trusted), "{trusted:?}");

    let operator = for_account(db, &cfg, &account("004"))
        .await
        .expect("TL6 lookup");
    assert_eq!(operator.role, Some(TropeRole::Operator), "{operator:?}");
    assert!(
        operator.allowed,
        "an operator has no bypass, just eligibility"
    );

    let active = for_account(db, &cfg, &account("005"))
        .await
        .expect("active role");
    assert_eq!(active.role, Some(TropeRole::Curator), "{active:?}");
    tdb.cleanup().await;
}
