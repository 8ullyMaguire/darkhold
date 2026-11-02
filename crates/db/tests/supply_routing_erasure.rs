//! M45-58 — §58.3's routing row survives erasure, and the account does not (both engines).
//!
//! The two clauses pull in opposite directions and the only compatible answer is the third one:
//!
//! - §58.3: *never delete a routing row* — an abandoned routing is the evidence the reader was asked
//!   and declined, and a deletion would hide that the offer was ever made.
//! - §51: erasure reaches every column that identifies a person.
//!
//! So the row stays and the link is replaced with a **per-row** key. The column is `NOT NULL`, and a
//! shared placeholder such as `'erased'` would quietly re-link every severed row of one person to the
//! others — a pseudonym is still personal data when it can be joined on. `erased:<row id>` is unique,
//! so two severed rows cannot be tied together, while `UNIQUE (account_id, work_id)` still holds.
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
        "darkhold-m45-58-{}-{}-{}",
        tag,
        uuid::Uuid::new_v4().simple(),
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let url = match std::env::var("DARKHOLD_TEST_PG_URL") {
        Ok(admin) => {
            let name = format!(
                "lh_er_{}_{}",
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

/// One account with one abandoned routing to one work.
async fn a_reader_who_declined(db: &Database) -> (String, String) {
    let account = uuid::Uuid::new_v4().to_string();
    let pseud = uuid::Uuid::new_v4().to_string();
    let work = uuid::Uuid::new_v4().to_string();
    exec(
        db,
        &format!(
            "INSERT INTO accounts (id, email, status, age_state, created_at, updated_at, permission_statement)
             VALUES ('{account}', 'routing-{account}@example.test', 'active', 'adult', '{NOW_STAMP}', '{NOW_STAMP}', 'minimal')"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at)
             VALUES ('{pseud}', '{account}', 'routing-{}', 'R', '{NOW_STAMP}', '{NOW_STAMP}')",
            &pseud[..8]
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at)
             VALUES ('{work}', '{pseud}', 'Routed Work', '{NOW_STAMP}', '{NOW_STAMP}')"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO supply_routing
                 (id, account_id, work_id, author_key, reason, created_at, completed_at, credit_bp)
             VALUES ('{}', '{account}', '{work}', 'author-key', 'recommended', '{NOW_STAMP}', NULL, NULL)",
            uuid::Uuid::new_v4()
        ),
    )
    .await;
    (account, work)
}

async fn routing_rows(db: &Database) -> Vec<(Option<String>, String, String)> {
    let sql = db.sql(
        "SELECT account_id, work_id, reason FROM supply_routing ORDER BY created_at ASC",
        "SELECT account_id::text, work_id::text, reason FROM supply_routing ORDER BY created_at ASC",
    );
    match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_as(&sql)
            .fetch_all(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("read routing"),
        darkhold_db::Backend::Postgres => sqlx::query_as(&sql)
            .fetch_all(db.postgres_pool().expect("postgres"))
            .await
            .expect("read routing"),
    }
}

#[tokio::test]
async fn the_routing_row_survives_erasure_without_the_account() {
    let db = connect("survives").await;
    let (account, work) = a_reader_who_declined(&db).await;

    let before = routing_rows(&db).await;
    assert_eq!(before.len(), 1, "one row before erasure");
    assert_eq!(before[0].0.as_deref(), Some(account.as_str()));

    erasure::erase_account(&db, &account)
        .await
        .expect("erase the account");

    let after = routing_rows(&db).await;
    assert_eq!(
        after.len(),
        1,
        "§58.3: the routing row is evidence the reader was asked, and it is never deleted"
    );
    let severed = after[0].0.as_deref().unwrap_or_default();
    assert_ne!(
        severed,
        account.as_str(),
        "§51: the row no longer names the reader"
    );
    assert!(
        severed.starts_with("erased:"),
        "the link is replaced with a per-row key, not left alone: {severed}"
    );
    assert_eq!(after[0].1, work, "the work it was routed to is still named");
    assert_eq!(after[0].2, "recommended", "and the reason is still legible");
}

#[tokio::test]
async fn severing_one_reader_leaves_another_readers_routing_alone() {
    let db = connect("others").await;
    let (erased, _work) = a_reader_who_declined(&db).await;
    let (kept, _other_work) = a_reader_who_declined(&db).await;

    erasure::erase_account(&db, &erased).await.expect("erase");

    let rows = routing_rows(&db).await;
    assert_eq!(rows.len(), 2, "both rows survive");
    let values: Vec<&str> = rows
        .iter()
        .map(|row| row.0.as_deref().unwrap_or_default())
        .collect();
    assert!(
        values.contains(&kept.as_str()),
        "the other reader is untouched: {values:?}"
    );
    assert!(
        values.iter().any(|value| value.starts_with("erased:")),
        "and the erased one is rekeyed: {values:?}"
    );
}

#[tokio::test]
async fn a_severed_row_does_not_block_a_fresh_routing_to_the_same_work() {
    let db = connect("reserves").await;
    let (account, work) = a_reader_who_declined(&db).await;
    erasure::erase_account(&db, &account).await.expect("erase");

    // A severed row's key is its own row id, so it cannot collide with a live account's routing to
    // the same work: `UNIQUE (account_id, work_id)` still admits both.
    exec(
        &db,
        &format!(
            "INSERT INTO supply_routing
                 (id, account_id, work_id, author_key, reason, created_at, completed_at, credit_bp)
             VALUES ('{}', '{account}', '{work}', 'author-key', 'recommended', '{NOW_STAMP}', NULL, NULL)",
            uuid::Uuid::new_v4()
        ),
    )
    .await;

    let rows = routing_rows(&db).await;
    assert_eq!(rows.len(), 2, "one severed, one live");
    let keys: Vec<&str> = rows
        .iter()
        .map(|row| row.0.as_deref().unwrap_or_default())
        .collect();
    assert!(
        keys.iter().any(|key| key.starts_with("erased:")) && keys.contains(&account.as_str()),
        "the severed row and the live one are different keys: {keys:?}"
    );
}
