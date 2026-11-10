//! The bare `account` spelling: erasure reaches it, and reaches it correctly (M45-64, second half).
//!
//! The erasure coverage check looked for `account_id`, `*_account` and `*_account_id`. The tree also uses a bare
//! `account`, on more tables than any other spelling, and the check believed its own convention: `trust_levels.account`
//! is a plain TEXT column with no `REFERENCES` clause, so an account deleted out from under it left the row behind. The
//! live instance had 189 such rows — standing held for people who no longer existed — while the suite that was supposed
//! to find this stayed green, because it never asked the tables what their columns were called.
//!
//! Twenty-two columns were invisible. Classifying them is half the work; this proves the other half, that the treatments
//! RUN. A classification is a string in a table until something executes it, and the whole point of this module is that the
//! leak it closes is silent.
//!
//! Run on SQLite by default; set `DARKHOLD_TEST_PG_URL` for the PostgreSQL arm.

use std::time::Duration;

use darkhold_db::{erasure, Database};

const NOW_STAMP: &str = "2026-11-02T10:00:00Z";

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
        "darkhold-m45-64b-{}-{}-{}",
        tag,
        uuid::Uuid::new_v4().simple(),
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let url = match std::env::var("DARKHOLD_TEST_PG_URL") {
        Ok(admin) => {
            let name = format!(
                "lh_ba_{}_{}",
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

/// One string column, one row.
async fn one(db: &Database, sqlite: &str, postgres: &str) -> Option<String> {
    let sql = db.sql(sqlite, postgres);
    match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar(&sql)
            .fetch_optional(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("read"),
        darkhold_db::Backend::Postgres => sqlx::query_scalar(&sql)
            .fetch_optional(db.postgres_pool().expect("pg"))
            .await
            .expect("read"),
    }
}

async fn a_deleted_account(db: &Database) -> String {
    let account = uuid::Uuid::new_v4().to_string();
    exec(
        db,
        &format!(
            "INSERT INTO accounts (id, email, status, age_state, created_at, updated_at, permission_statement)
             VALUES ('{account}', 'bare-{account}@example.test', 'active', 'adult', '{NOW_STAMP}', '{NOW_STAMP}', 'minimal')"
        ),
    )
    .await;
    account
}

#[tokio::test]
async fn erasure_reaches_every_bare_account_column() {
    let db = connect("all").await;
    let account = a_deleted_account(&db).await;

    // 1. Rows that must GO. `trust_levels` is the one the live instance proved: 189 orphans for deleted accounts.
    exec(
        &db,
        &format!(
            "INSERT INTO trust_levels (account, level, computed_at, basis)
             VALUES ('{account}', 2, '{NOW_STAMP}', 'tests')"
        ),
    )
    .await;
    exec(
        &db,
        &format!("INSERT INTO wishlists (account, is_public) VALUES ('{account}', 1)"),
    )
    .await;
    exec(
        &db,
        &format!(
            "INSERT INTO privacy_requests (id, account, kind, state, requested_at)
         VALUES ('{}', '{account}', 'delete', 'pending', '{NOW_STAMP}')",
            uuid::Uuid::new_v4()
        ),
    )
    .await;

    // 2. Rows that must SURVIVE, unlinkable. `payments` and `privacy_requests` have a single-column `id`;
    //    `credit_entries` has no primary key at all, so its key is (transaction_id, bucket) — the reason `Rekey`
    //    carries the expression instead of assuming `id`.
    let payment = uuid::Uuid::new_v4().to_string();
    exec(
        &db,
        &format!(
            "INSERT INTO payments (id, account, subject_kind, subject_id, amount_minor, currency, state, idempotency, created_at)
             VALUES ('{payment}', '{account}', 'extension', 'sub-1', 500, 'EUR', 'captured', 'bare-{payment}', '{NOW_STAMP}')"
        ),
    )
    .await;
    let request = one(
        &db,
        &format!("SELECT id FROM privacy_requests WHERE account = '{account}'"),
        &format!("SELECT id FROM privacy_requests WHERE account = '{account}'"),
    )
    .await
    .expect("the request row exists before erasure");
    let transaction = uuid::Uuid::new_v4().to_string();
    exec(
        &db,
        &format!(
            "INSERT INTO credit_transactions (id, type, idempotency_key, reference, created_at)
             VALUES ('{transaction}', 'grant', 'bare-{transaction}', 'grant', '{NOW_STAMP}')"
        ),
    )
    .await;
    exec(
        &db,
        &format!(
            "INSERT INTO credit_entries (transaction_id, account, bucket, amount_bp, created_at)
             VALUES ('{transaction}', '{account}', 'earned', 100, '{NOW_STAMP}')"
        ),
    )
    .await;

    // 3. Erase. Everything above is reached by the list, not by a cascade: none of these columns has a REFERENCES clause.
    erasure::erase_account(&db, &account)
        .await
        .expect("erase the account");

    // 4. The deletes happened.
    assert_eq!(
        one(&db, &format!("SELECT account FROM trust_levels WHERE account = '{account}'"),
            &format!("SELECT account FROM trust_levels WHERE account = '{account}'"))
            .await,
        None,
        "standing does not outlive the person — this is the row the live instance left behind 189 times"
    );
    assert_eq!(
        one(
            &db,
            &format!("SELECT account FROM wishlists WHERE account = '{account}'"),
            &format!("SELECT account FROM wishlists WHERE account = '{account}'")
        )
        .await,
        None,
        "a wishlist is a preference and goes with the person"
    );

    // 5. The severance happened, per row, and it is the row's own identity — not a shared placeholder that would
    //    re-link every severed row of one person to the others.
    assert_eq!(
        one(
            &db,
            &format!("SELECT account FROM payments WHERE id = '{payment}'"),
            &format!("SELECT account FROM payments WHERE id = '{payment}'")
        )
        .await,
        Some(format!("erased:{payment}")),
        "a payment survives its payer with the identity severed to the row's own id"
    );
    assert_eq!(
        one(
            &db,
            &format!("SELECT account FROM privacy_requests WHERE id = '{request}'"),
            &format!("SELECT account FROM privacy_requests WHERE id = '{request}'")
        )
        .await,
        Some(format!("erased:{request}")),
        "the request survives as the instance's evidence that it honoured it"
    );
    assert_eq!(
        one(
            &db,
            &format!("SELECT account FROM credit_entries WHERE transaction_id = '{transaction}'"),
            &format!("SELECT account FROM credit_entries WHERE transaction_id = '{transaction}'")
        )
        .await,
        Some(format!("erased:{transaction}:earned")),
        "a ledger line survives with its amount; its key is (transaction_id, bucket) because the table has no primary key"
    );

    // 6. And nothing anywhere still names the account. This is the assertion a per-column test cannot make: the leak was
    //    in the columns nobody enumerated, so the check that matters is over the whole list.
    for column in erasure::FK_FREE_ACCOUNT_COLUMNS {
        let found = one(
            &db,
            &format!(
                "SELECT {} FROM {} WHERE {} = '{account}' LIMIT 1",
                column.column, column.table, column.column
            ),
            &format!(
                "SELECT {}::text FROM {} WHERE {}::text = '{account}' LIMIT 1",
                column.column, column.table, column.column
            ),
        )
        .await;
        let lookup_failed_is_a_failure = format!(
            "{} . {} still answers for an erased account — either the treatment did not run or the column is not in the list",
            column.table, column.column
        );
        // A table with no rows of this account answers None, which is the pass; a table whose statement does not compile
        // panics in `one`, which is the point of not swallowing it — a checklist entry that cannot query its own table
        // would otherwise read exactly like a clean one.
        assert_eq!(found, None, "{lookup_failed_is_a_failure}");
    }
}
