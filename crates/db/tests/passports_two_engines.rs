//! M69-03 — passports through the store, on whichever engine `DARKHOLD_TEST_PG_URL` names.
//!
//! Four claims, and each is a way the bootstrap can go wrong:
//!
//! * a verified passport seeds `imported` signals, not `organic` ones (§49.7's distinction is the point of the feature);
//! * a **tampered** passport creates no row and no signal — verification runs before anything is written;
//! * re-presenting a newer document from the same origin **replaces** the older taste instead of adding to it, because two
//!   versions of one prior are not two priors;
//! * withdrawing removes the receipt *and* the signals it seeded.

use std::collections::BTreeMap;

use darkhold_db::{passports, Backend, Database};
use darkhold_domain::passport::{self, Contributions, Passport, SignedPassport};

const NOW: &str = "2026-11-07T00:00:00Z";
/// A fixed key, so the test's documents are reproducible and nobody has to read a generated one back.
const KEY: &str = "1111111111111111111111111111111111111111111111111111111111111111";

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
            "darkhold-passport-{}-{}",
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
    let name = format!("lh_pass_{}", uuid::Uuid::new_v4().simple());
    let admin_db = Database::connect(&make_config(admin.clone()))
        .await
        .expect("admin");
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

fn passport(tags: &[(&str, u32)]) -> Passport {
    Passport {
        version: passport::VERSION,
        origin: "https://origin.example".to_owned(),
        subject: "a-reader".to_owned(),
        issued_at: "2026-11-01T00:00:00Z".to_owned(),
        taste: tags
            .iter()
            .map(|(tag, weight)| ((*tag).to_owned(), *weight))
            .collect::<BTreeMap<String, u32>>(),
        favourite_sources: vec!["https://archiveofourown.org/users/x".to_owned()],
        contributions: Contributions {
            works: 3,
            revisions: 9,
            comments: 40,
        },
        affinity_bp: Some(7_800),
    }
}

async fn signals(db: &Database, account: &str) -> Vec<(String, String, f64)> {
    let rows: Vec<(String, String, f64)> = match db.backend() {
        Backend::Sqlite => sqlx::query_as(
            "SELECT dimension_key, origin, signal_value FROM taste_signals
                  WHERE account_id = ? ORDER BY dimension_key",
        )
        .bind(account)
        .fetch_all(db.sqlite_pool().expect("sqlite"))
        .await
        .expect("signals"),
        Backend::Postgres => sqlx::query_as(
            "SELECT dimension_key, origin, signal_value::double precision FROM taste_signals
                  WHERE account_id = $1::uuid ORDER BY dimension_key",
        )
        .bind(account)
        .fetch_all(db.postgres_pool().expect("postgres"))
        .await
        .expect("signals"),
    };
    rows
}

async fn account(db: &Database, handle: &str) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO accounts (id, email, status, age_state, created_at, updated_at, permission_statement)
                 VALUES (?, ?, 'active', 'adult', ?, ?, 'minimal')",
            )
            .bind(&id)
            .bind(format!("{handle}@example.test"))
            .bind(NOW)
            .bind(NOW)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("account");
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO accounts (id, email, status, age_state, created_at, updated_at, permission_statement)
                 VALUES ($1::uuid, $2, 'active', 'adult', $3::timestamptz, $3::timestamptz, 'minimal')",
            )
            .bind(&id)
            .bind(format!("{handle}@example.test"))
            .bind(NOW)
            .execute(db.postgres_pool().expect("postgres"))
            .await
            .expect("account");
        }
    }
    id
}

#[tokio::test]
async fn a_verified_passport_seeds_imported_signals_and_a_tampered_one_seeds_nothing() {
    let db = connect().await;
    let db = &db;
    let reader = account(db, "passport-reader").await;

    let document = passport(&[("angst", 5_000), ("fluff", 3_000), ("au", 2_000)]);
    let signed = passport::sign_with_hex(&document, KEY).expect("sign");
    let receipt = passports::import(db, &reader, &signed, NOW)
        .await
        .expect("import");
    assert_eq!(receipt.origin, "https://origin.example");
    assert_eq!(receipt.signals, 3, "one signal per tag");
    assert_eq!(receipt.taste_bp.get("angst"), Some(&5_000));

    let seeded = signals(db, &reader).await;
    assert_eq!(seeded.len(), 3);
    for (dimension, origin, value) in &seeded {
        assert!(
            dimension.starts_with("tag:"),
            "a passport's evidence lands on tag dimensions: {dimension}"
        );
        assert_eq!(
            origin, "imported",
            "the passport is somebody else's measurement: {origin} would let it outrank local evidence"
        );
        assert!(*value > 0.0 && *value <= 1.0, "{value} is not a share");
    }

    // A tampered document: the signature no longer covers it, and nothing is written.
    let other = account(db, "passport-forger").await;
    let mut forged: SignedPassport = signed.clone();
    forged.passport.taste.insert("angst".to_owned(), 9_900);
    let error = passports::import(db, &other, &forged, NOW)
        .await
        .expect_err("a forged passport is refused");
    assert!(error.to_string().contains("signature"), "{error}");
    assert!(
        signals(db, &other).await.is_empty(),
        "verification runs before any write"
    );
    assert!(
        passports::for_account(db, &other)
            .await
            .expect("receipts")
            .is_empty(),
        "a refused passport leaves no receipt"
    );
}

#[tokio::test]
async fn re_presenting_a_newer_document_replaces_the_old_taste_and_withdrawing_removes_both() {
    let db = connect().await;
    let db = &db;
    let reader = account(db, "passport-mover").await;

    let first = passport::sign_with_hex(&passport(&[("angst", 6_000), ("hurt", 4_000)]), KEY)
        .expect("sign");
    passports::import(db, &reader, &first, NOW)
        .await
        .expect("first import");
    assert_eq!(signals(db, &reader).await.len(), 2);

    let second =
        passport::sign_with_hex(&passport(&[("smut", 7_000), ("au", 3_000)]), KEY).expect("sign");
    let receipt = passports::import(db, &reader, &second, NOW)
        .await
        .expect("second import");
    assert_eq!(receipt.signals, 2);

    let seeded = signals(db, &reader).await;
    assert_eq!(
        seeded.len(),
        2,
        "two versions of one prior are not two priors: {seeded:?}"
    );
    assert!(
        seeded
            .iter()
            .all(|(dimension, _, _)| dimension != "tag:angst" && dimension != "tag:hurt"),
        "the older document's taste is gone, not merely outweighed: {seeded:?}"
    );

    let receipts = passports::for_account(db, &reader).await.expect("receipts");
    assert_eq!(receipts.len(), 1, "one origin, one receipt");

    assert!(
        passports::revoke(db, &reader, "https://origin.example")
            .await
            .expect("revoke"),
        "revoking an existing passport reports that it removed something"
    );
    assert!(
        signals(db, &reader).await.is_empty(),
        "the taste goes with the receipt"
    );
    assert!(passports::for_account(db, &reader)
        .await
        .expect("receipts")
        .is_empty());
    assert!(
        !passports::revoke(db, &reader, "https://origin.example")
            .await
            .expect("revoke again"),
        "a second withdrawal is a no-op, not a claim that something was removed"
    );
}
