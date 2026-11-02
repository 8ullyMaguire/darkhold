//! M45-30 — §61 cross-language supply: the report, and funding a translation, on both engines.
//!
//! What only a database can get wrong:
//!
//! 1. Supply counts by **primary subtag** (`pt-BR` and `pt` are one bucket) and totals every work.
//! 2. An empty catalogue is a zeroed report, not an error.
//! 3. A `translation: yes` work funds, and the bounty lands in `bounties` with
//!    `job_kind = 'translation'` and terms naming both languages.
//! 4. `ask` refuses — funding it would spend the author's silence.
//! 5. `no` and `unstated` refuse, and the statement is named.
//! 6. An account that cannot cover the amount is refused and **nothing is written**.
//! 7. The listing is open translation bounties only — a closed one and a different `job_kind` are
//!    both excluded.
//!
//! Run on SQLite by default; set `DARKHOLD_TEST_PG_URL` for the PostgreSQL arm.

use std::time::Duration;

use darkhold_db::translation::{fund_translation, language_supply, translation_bounties, FundOutcome};
use darkhold_db::{Backend, Database};
use darkhold_domain::permission::{Permission, PermissionStatement};

const T0: &str = "2026-03-01T00:00:00Z";

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
        "darkhold-translations-{}-{}-{}",
        tag,
        uuid::Uuid::new_v4(),
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let url = match std::env::var("DARKHOLD_TEST_PG_URL") {
        Ok(admin) => {
            let name = format!(
                "lh_translations_{}_{}",
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
            let (prefix, _) = admin.rsplit_once('/').expect("admin URL ends in a database");
            format!("{prefix}/{name}")
        }
        Err(_) => format!("sqlite://{}/darkhold.sqlite?mode=rwc", dir.display()),
    };
    let db = Database::connect(&make_config(url)).await.expect("connect");
    db.migrate().await.expect("migrate");
    db
}

async fn who(db: &Database, tag: &str) -> (String, String) {
    let account = uuid::Uuid::new_v4().to_string();
    let pseud = uuid::Uuid::new_v4().to_string();
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query("INSERT INTO accounts (id, email, created_at, updated_at) VALUES (?, ?, ?, ?)")
                .bind(&account)
                .bind(format!("{tag}@example.invalid"))
                .bind(T0)
                .bind(T0)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("account");
            sqlx::query(
                "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(&pseud)
            .bind(&account)
            .bind(tag)
            .bind(tag)
            .bind(T0)
            .bind(T0)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("pseud");
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO accounts (id, email, created_at, updated_at)
                 VALUES ($1::uuid, $2, $3::timestamptz, $3::timestamptz)",
            )
            .bind(uuid::Uuid::parse_str(&account).expect("uuid"))
            .bind(format!("{tag}@example.invalid"))
            .bind(T0)
            .execute(db.postgres_pool().expect("postgres"))
            .await
            .expect("account");
            sqlx::query(
                "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at)
                 VALUES ($1::uuid, $2::uuid, $3, $3, $4::timestamptz, $4::timestamptz)",
            )
            .bind(uuid::Uuid::parse_str(&pseud).expect("uuid"))
            .bind(uuid::Uuid::parse_str(&account).expect("uuid"))
            .bind(tag)
            .bind(T0)
            .execute(db.postgres_pool().expect("postgres"))
            .await
            .expect("pseud");
        }
    }
    (account, pseud)
}

async fn work(db: &Database, pseud: &str, language: &str) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO works (id, owner_pseud_id, title, language, created_at, updated_at,
                                    generated_content_posture)
                 VALUES (?, ?, 'A work', ?, ?, ?, 'forbid')",
            )
            .bind(&id)
            .bind(pseud)
            .bind(language)
            .bind(T0)
            .bind(T0)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("work");
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO works (id, owner_pseud_id, title, language, created_at, updated_at,
                                    generated_content_posture)
                 VALUES ($1::uuid, $2::uuid, 'A work', $3, $4::timestamptz, $4::timestamptz, 'forbid')",
            )
            .bind(uuid::Uuid::parse_str(&id).expect("uuid"))
            .bind(uuid::Uuid::parse_str(pseud).expect("uuid"))
            .bind(language)
            .bind(T0)
            .execute(db.postgres_pool().expect("postgres"))
            .await
            .expect("work");
        }
    }
    id
}

/// Set the work's translation statement through the real §40.2 path.
async fn set_translation(db: &Database, work_id: &str, permission: Permission) {
    let mut stmt = PermissionStatement::unstated();
    stmt.translation = permission;
    darkhold_db::permission::set_work_permission_statement(db, work_id, &stmt)
        .await
        .expect("statement");
}

/// Give an account `amount` basis points in the `earned` bucket.
async fn credit(db: &Database, account: &str, amount: i64) {
    let txn = uuid::Uuid::new_v4().to_string();
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO credit_transactions (id, type, idempotency_key, reference, created_at)
                 VALUES (?, 'grant', ?, 'test', ?)",
            )
            .bind(&txn)
            .bind(&txn)
            .bind(T0)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("txn");
            sqlx::query(
                "INSERT INTO credit_entries (transaction_id, account, bucket, amount_bp, created_at)
                 VALUES (?, ?, 'earned', ?, ?)",
            )
            .bind(&txn)
            .bind(account)
            .bind(amount)
            .bind(T0)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("entry");
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO credit_transactions (id, type, idempotency_key, reference, created_at)
                 VALUES ($1, 'grant', $2, 'test', $3::timestamptz)",
            )
            .bind(&txn)
            .bind(&txn)
            .bind(T0)
            .execute(db.postgres_pool().expect("postgres"))
            .await
            .expect("txn");
            sqlx::query(
                "INSERT INTO credit_entries (transaction_id, account, bucket, amount_bp, created_at)
                 VALUES ($1, $2, 'earned', $3, $4::timestamptz)",
            )
            .bind(&txn)
            .bind(account)
            .bind(amount)
            .bind(T0)
            .execute(db.postgres_pool().expect("postgres"))
            .await
            .expect("entry");
        }
    }
}

#[tokio::test]
async fn supply_counts_by_primary_subtag_and_totals_every_work() {
    let db = connect("supply").await;
    let (_, pseud) = who(&db, "author").await;
    work(&db, &pseud, "en").await;
    work(&db, &pseud, "en").await;
    work(&db, &pseud, "pt-BR").await;
    work(&db, &pseud, "pt").await;
    work(&db, &pseud, "es-419").await;

    let supply = language_supply(&db).await.expect("supply");
    assert_eq!(supply.total, 5, "every work is counted once");
    assert_eq!(supply.by_language.get("pt"), Some(&2), "pt-BR and pt are one bucket");
    assert_eq!(supply.by_language.get("es"), Some(&1));
    assert_eq!(supply.by_language.get("en"), Some(&2));
}

#[tokio::test]
async fn an_empty_catalogue_is_a_zeroed_report_not_an_error() {
    let db = connect("empty").await;
    let supply = language_supply(&db).await.expect("supply");
    assert_eq!(supply.total, 0);
    assert!(supply.by_language.is_empty());
}

#[tokio::test]
async fn a_yes_statement_funds_a_translation_bounty() {
    let db = connect("fund").await;
    let (patron, pseud) = who(&db, "patron").await;
    let work_id = work(&db, &pseud, "en").await;
    set_translation(&db, &work_id, Permission::Yes).await;
    credit(&db, &patron, 50_000).await;

    let outcome = fund_translation(&db, &work_id, "pt", &patron, 10_000)
        .await
        .expect("fund");
    let FundOutcome::Funded { bounty_id } = outcome else {
        panic!("expected a funded bounty, got {outcome:?}");
    };

    let listed = translation_bounties(&db, 10).await.expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["id"], serde_json::json!(bounty_id));
    assert_eq!(listed[0]["state"], serde_json::json!("open"));
    let terms: serde_json::Value =
        serde_json::from_str(listed[0]["terms"].as_str().expect("terms string")).expect("json");
    assert_eq!(terms["work_id"], serde_json::json!(work_id));
    assert_eq!(terms["source_language"], serde_json::json!("en"));
    assert_eq!(terms["target_language"], serde_json::json!("pt"));
}

#[tokio::test]
async fn ask_refuses_because_funding_it_would_spend_the_authors_silence() {
    let db = connect("ask").await;
    let (patron, pseud) = who(&db, "patron").await;
    let work_id = work(&db, &pseud, "en").await;
    set_translation(&db, &work_id, Permission::Ask).await;
    credit(&db, &patron, 50_000).await;

    assert_eq!(
        fund_translation(&db, &work_id, "pt", &patron, 1_000)
            .await
            .expect("fund"),
        FundOutcome::MustAsk
    );
    assert!(translation_bounties(&db, 10).await.expect("list").is_empty());
}

#[tokio::test]
async fn no_and_unstated_refuse_and_name_the_statement() {
    let db = connect("forbidden").await;
    let (patron, pseud) = who(&db, "patron").await;
    credit(&db, &patron, 50_000).await;

    for (permission, expected) in [
        (Permission::No, "no"),
        (Permission::Unstated, "unstated"),
    ] {
        let work_id = work(&db, &pseud, "en").await;
        set_translation(&db, &work_id, permission).await;
        assert_eq!(
            fund_translation(&db, &work_id, "pt", &patron, 1_000)
                .await
                .expect("fund"),
            FundOutcome::Forbidden { statement: expected },
            "{permission:?} must refuse and name the statement"
        );
    }
    assert!(
        translation_bounties(&db, 10).await.expect("list").is_empty(),
        "a refused funding wrote a bounty"
    );
}

#[tokio::test]
async fn an_account_without_the_balance_is_refused_and_writes_nothing() {
    let db = connect("poor").await;
    let (patron, pseud) = who(&db, "patron").await;
    let work_id = work(&db, &pseud, "en").await;
    set_translation(&db, &work_id, Permission::Yes).await;
    credit(&db, &patron, 500).await;

    assert_eq!(
        fund_translation(&db, &work_id, "pt", &patron, 10_000)
            .await
            .expect("fund"),
        FundOutcome::InsufficientBalance {
            available: 500,
            required: 10_000
        }
    );
    assert!(translation_bounties(&db, 10).await.expect("list").is_empty());
}

#[tokio::test]
async fn permission_is_read_before_the_balance() {
    // An account with no credits funding more than it holds, on a work whose author said `no`.
    // If the balance were read first the answer would be `InsufficientBalance`; the order is the rule,
    // because a balance refusal tells a patron to top up for something the author forbade.
    let db = connect("order").await;
    let (patron, pseud) = who(&db, "patron").await;
    let work_id = work(&db, &pseud, "en").await;
    set_translation(&db, &work_id, Permission::No).await;

    assert_eq!(
        fund_translation(&db, &work_id, "pt", &patron, 999_999_999)
            .await
            .expect("fund"),
        FundOutcome::Forbidden { statement: "no" }
    );
    assert!(translation_bounties(&db, 10).await.expect("list").is_empty());
}

#[tokio::test]
async fn the_listing_is_open_translation_bounties_only() {
    let db = connect("listing").await;
    let (patron, pseud) = who(&db, "patron").await;
    let work_id = work(&db, &pseud, "en").await;
    set_translation(&db, &work_id, Permission::Yes).await;
    credit(&db, &patron, 100_000).await;

    fund_translation(&db, &work_id, "pt", &patron, 1_000)
        .await
        .expect("fund");
    // A different job kind, and a closed translation bounty, must both stay out.
    darkhold_db::economy::create_bounty(
        &db,
        &uuid::Uuid::new_v4().to_string(),
        &patron,
        "podfic",
        "{}",
        1_000,
        T0,
    )
    .await
    .expect("other bounty");
    let closed = uuid::Uuid::new_v4().to_string();
    darkhold_db::economy::create_bounty(&db, &closed, &patron, "translation", "{}", 1_000, T0)
        .await
        .expect("closed bounty");
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query("UPDATE bounties SET state = 'claimed' WHERE id = ?")
                .bind(&closed)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("close");
        }
        Backend::Postgres => {
            sqlx::query("UPDATE bounties SET state = 'claimed' WHERE id = $1")
                .bind(&closed)
                .execute(db.postgres_pool().expect("postgres"))
                .await
                .expect("close");
        }
    }

    let listed = translation_bounties(&db, 50).await.expect("list");
    assert_eq!(listed.len(), 1, "only the open translation bounty: {listed:?}");
}
