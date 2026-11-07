//! The payment store on both engines (M16-11).
//!
//! The gallery row taught three dialect lessons the hard way — `uuid = text` joins, `INT4` decode, `SUM` widening — and this module has the
//! same three shapes. It is checked on PostgreSQL rather than assumed, because a purchase that works on SQLite and 500s on the engine the
//! product runs is not a feature.
//!
//! Runs on whichever engine `DARKHOLD_TEST_PG_URL` names.

use darkhold_db::payments::{
    capture, enabled_price, payment, record_payment, record_refund, seller_net, set_price,
    set_tax_profile, tax_profile, NewPayment,
};
use darkhold_db::Database;
use darkhold_domain::payments::TaxProfile;

const ACCOUNT: &str = "01955100-0000-4000-8000-000000f00001";
const SELLER: &str = "01955100-0000-4000-8000-000000f00002";
const PACKAGE: &str = "payment-package";

fn make_config(url: String) -> darkhold_db::DatabaseConfig {
    let mut config = darkhold_db::DatabaseConfig::new(url);
    config.max_connections = 2;
    config.acquire_timeout = std::time::Duration::from_secs(60);
    config
}

async fn connect() -> Database {
    let Ok(admin) = std::env::var("DARKHOLD_TEST_PG_URL") else {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "darkhold-payments-2e-{}-{}",
            uuid::Uuid::new_v4().simple(),
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let db = Database::connect(&make_config(format!(
            "sqlite://{}/darkhold.sqlite?mode=rwc",
            dir.display()
        )))
        .await
        .expect("connect sqlite");
        db.migrate().await.expect("migrate");
        return db;
    };
    let name = format!("lh_pay_{}", uuid::Uuid::new_v4().simple());
    let admin_db = Database::connect(&make_config(admin.clone()))
        .await
        .expect("connect admin");
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(admin_db.postgres_pool().expect("postgres handle"))
        .await
        .expect("create database");
    let url = admin
        .rsplit_once('/')
        .map(|(prefix, _)| format!("{prefix}/{name}"))
        .expect("a database url has a path");
    let db = Database::connect(&make_config(url)).await.expect("connect");
    db.migrate().await.expect("migrate");
    db
}

async fn exec(db: &Database, sqlite: &str, postgres: &str) {
    match db.backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(sqlite)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await
                .expect("fixture statement (sqlite)");
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(postgres)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await
                .expect("fixture statement (postgres)");
        }
    }
}

async fn fixture(db: &Database) {
    for account in [ACCOUNT, SELLER] {
        exec(
            db,
            &format!(
                "INSERT INTO accounts (id, email, created_at, updated_at)
                 VALUES ('{account}', '{account}@example.invalid', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
            ),
            &format!(
                "INSERT INTO accounts (id, email, created_at, updated_at)
                 VALUES ('{account}', '{account}@example.invalid', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
            ),
        )
        .await;
    }
    let document = r#"{"id":"payment-package","version":1,"category":"integration","entrypoint":"main.wasm","required_host_api_version":1,"permissions":[],"resource_limits":{"memory_mib":16,"fuel":1000,"host_calls_per_run":8},"supported_surfaces":["library"],"license":"MIT","network_allowlist":[],"pricing":"paid"}"#;
    exec(
        db,
        &format!(
            "INSERT INTO extension_manifests (id, version, document, submitted_by, state, created_at)
             VALUES ('{PACKAGE}', '1.0.0', '{document}', '{SELLER}', 'approved', '2026-11-01T00:00:00Z')"
        ),
        &format!(
            "INSERT INTO extension_manifests (id, version, document, submitted_by, state, created_at)
             VALUES ('{PACKAGE}', '1.0.0', '{document}', '{SELLER}', 'approved', '2026-11-01T00:00:00Z')"
        ),
    )
    .await;
}

#[tokio::test]
async fn a_price_is_stored_and_read_back_on_both_engines() {
    let db = connect().await;
    fixture(&db).await;
    assert_eq!(enabled_price(&db, PACKAGE).await.expect("price"), None);

    set_price(&db, PACKAGE, 1_000, "EUR", true, SELLER)
        .await
        .expect("set price");
    assert_eq!(
        enabled_price(&db, PACKAGE).await.expect("price"),
        Some((1_000, "EUR".to_owned())),
        "the boolean is stored as an integer so one decode serves both engines"
    );

    set_price(&db, PACKAGE, 1_000, "EUR", false, SELLER)
        .await
        .expect("disable");
    assert_eq!(enabled_price(&db, PACKAGE).await.expect("price"), None);
}

#[tokio::test]
async fn a_payment_is_idempotent_captured_once_and_refunded_within_its_amount() {
    let db = connect().await;
    fixture(&db).await;

    let first = record_payment(
        &db,
        &NewPayment {
            account: ACCOUNT,
            subject_kind: "extension",
            subject_id: PACKAGE,
            subject_version: Some("1.0.0"),
            amount_minor: 1_000,
            currency: "EUR",
            tax_minor: 160,
            idempotency: "extension:buyer:package",
            processor: "manual",
        },
    )
    .await
    .expect("record");
    assert_eq!(first.state, darkhold_domain::payments::PaymentState::Pending);

    // The idempotency key is the guard: a retry returns the same row.
    let retried = record_payment(
        &db,
        &NewPayment {
            account: ACCOUNT,
            subject_kind: "extension",
            subject_id: PACKAGE,
            subject_version: Some("1.0.0"),
            amount_minor: 1_000,
            currency: "EUR",
            tax_minor: 160,
            idempotency: "extension:buyer:package",
            processor: "manual",
        },
    )
    .await
    .expect("retry");
    assert_eq!(retried.id, first.id, "a retry must not create a second payment");

    assert!(capture(&db, &first.id, "manual:test").await.expect("capture"));
    assert!(
        !capture(&db, &first.id, "manual:second").await.expect("capture twice"),
        "capturing twice is a no-op, not a second charge"
    );
    let captured = payment(&db, &first.id).await.expect("read").expect("row");
    assert_eq!(captured.state, darkhold_domain::payments::PaymentState::Captured);
    assert_eq!(captured.tax_minor, 160);

    // `seller_net` joins the manifest; on PostgreSQL `submitted_by` is a uuid column and the comparison is against a text bind, which is
    // exactly the shape that needed a cast in the gallery module.
    assert_eq!(seller_net(&db, SELLER, "EUR").await.expect("net"), 1_000);

    assert!(record_refund(&db, &first.id, 400, "goodwill", "op")
        .await
        .expect("partial"));
    assert_eq!(seller_net(&db, SELLER, "EUR").await.expect("net"), 600);

    // Past the captured amount is refused by the statement itself, not only by the caller.
    assert!(!record_refund(&db, &first.id, 601, "too much", "op")
        .await
        .expect("over-refund is false, not an error"));
    assert!(record_refund(&db, &first.id, 600, "the rest", "op")
        .await
        .expect("final"));
    let settled = payment(&db, &first.id).await.expect("read").expect("row");
    assert_eq!(settled.state, darkhold_domain::payments::PaymentState::Refunded);
    assert_eq!(settled.refunded_minor, 1_000);
    assert!(
        !record_refund(&db, &first.id, 1, "again", "op").await.expect("closed"),
        "a refunded payment accepts nothing further"
    );
}

#[tokio::test]
async fn the_tax_profile_round_trips() {
    let db = connect().await;
    assert_eq!(tax_profile(&db).await.expect("read"), None);
    let profile = TaxProfile {
        legal_name: "Archive Ltd".to_owned(),
        country: "DE".to_owned(),
        tax_id: Some("DE123456789".to_owned()),
        rate_bp: 1_900,
        invoice_prefix: "DH".to_owned(),
    };
    set_tax_profile(&db, &profile).await.expect("write");
    assert_eq!(tax_profile(&db).await.expect("read"), Some(profile.clone()));

    // One row: setting it again replaces rather than appending.
    let updated = TaxProfile {
        rate_bp: 700,
        ..profile
    };
    set_tax_profile(&db, &updated).await.expect("write again");
    assert_eq!(tax_profile(&db).await.expect("read"), Some(updated));
}
