//! The gallery store on both engines (M16-08).
//!
//! Every statement in `darkhold_db::gallery` is written twice, and the two dialects disagree about exactly the things this module leans
//! on: `COUNT(*)` and `SUM(stars)` widen to `BIGINT`/`NUMERIC` on PostgreSQL and stay `INTEGER` on SQLite, and the partial unique index
//! that makes one active install per scope has to be honoured by the upsert's conflict target on both. A run on SQLite alone would pass
//! while the PostgreSQL arm 500s on the first rating.
//!
//! Runs on whichever engine `DARKHOLD_TEST_PG_URL` names: SQLite when unset, PostgreSQL when set.

use darkhold_db::gallery::{
    install, install_count, my_installations, published_versions, rate, reviews, score, set_version,
    uninstall,
};
use darkhold_db::Database;
use darkhold_domain::gallery::UpdatePolicy;

const ACCOUNT: &str = "01955100-0000-4000-8000-000000ec0001";
const PSEUD: &str = "01955100-0000-4000-8000-000000ed0001";
const PACKAGE: &str = "two-engine-package";

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
            "darkhold-gallery-2e-{}-{}",
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
    let name = format!("lh_gallery_{}", uuid::Uuid::new_v4().simple());
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
    exec(
        db,
        &format!(
            "INSERT INTO accounts (id, email, created_at, updated_at)
             VALUES ('{ACCOUNT}', 'gallery-2e@example.invalid', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        ),
        &format!(
            "INSERT INTO accounts (id, email, created_at, updated_at)
             VALUES ('{ACCOUNT}', 'gallery-2e@example.invalid', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at)
             VALUES ('{PSEUD}', '{ACCOUNT}', 'gallery-pseud', 'Gallery', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        ),
        &format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at)
             VALUES ('{PSEUD}', '{ACCOUNT}', 'gallery-pseud', 'Gallery', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        ),
    )
    .await;
    let document = r#"{"id":"two-engine-package","version":1,"category":"integration","entrypoint":"main.wasm","required_host_api_version":1,"permissions":[],"resource_limits":{"memory_mib":16,"fuel":1000,"host_calls_per_run":8},"supported_surfaces":["library"],"license":"MIT","network_allowlist":[],"pricing":"free"}"#;
    exec(
        db,
        &format!(
            "INSERT INTO extension_manifests (id, version, document, submitted_by, state, created_at)
             VALUES ('{PACKAGE}', '1.0.0', '{document}', '{ACCOUNT}', 'approved', '2026-11-01T00:00:00Z')"
        ),
        &format!(
            "INSERT INTO extension_manifests (id, version, document, submitted_by, state, created_at)
             VALUES ('{PACKAGE}', '1.0.0', '{document}', '{ACCOUNT}', 'approved', '2026-11-01T00:00:00Z')"
        ),
    )
    .await;
}

#[tokio::test]
async fn installs_are_idempotent_and_counts_are_a_census() {
    let db = connect().await;
    fixture(&db).await;

    let (outcome, installation) = install(&db, PSEUD, PACKAGE, "1.0.0", UpdatePolicy::Pinned)
        .await
        .expect("install");
    assert_eq!(outcome, darkhold_domain::gallery::InstallOutcome::Installed);
    assert_eq!(installation.version, "1.0.0");
    assert_eq!(install_count(&db, PACKAGE).await.expect("count"), 1);

    // The idempotent case, on this engine, is the one the upsert's `WHERE` clause carries.
    let (outcome, _) = install(&db, PSEUD, PACKAGE, "1.0.0", UpdatePolicy::Pinned)
        .await
        .expect("second install");
    assert_eq!(
        outcome,
        darkhold_domain::gallery::InstallOutcome::AlreadyInstalled
    );
    assert_eq!(install_count(&db, PACKAGE).await.expect("count"), 1);

    assert!(uninstall(&db, PSEUD, PACKAGE).await.expect("uninstall"));
    assert_eq!(install_count(&db, PACKAGE).await.expect("count"), 0);

    // And the reactivation path, which is a different branch of the same statement.
    let (outcome, _) = install(&db, PSEUD, PACKAGE, "1.0.0", UpdatePolicy::Minor)
        .await
        .expect("reinstall");
    assert_eq!(outcome, darkhold_domain::gallery::InstallOutcome::Installed);
    assert_eq!(install_count(&db, PACKAGE).await.expect("count"), 1);
}

#[tokio::test]
async fn an_update_moves_the_version_and_the_scope_list_reads_it_back() {
    let db = connect().await;
    fixture(&db).await;
    install(&db, PSEUD, PACKAGE, "1.0.0", UpdatePolicy::Pinned)
        .await
        .expect("install");

    assert!(set_version(&db, PSEUD, PACKAGE, "1.0.0", UpdatePolicy::Minor)
        .await
        .expect("set"));
    let listings = my_installations(&db, PSEUD).await.expect("list");
    assert_eq!(listings.len(), 1, "{listings:?}");
    assert_eq!(listings[0]["update_policy"], "minor", "{listings:?}");

    assert_eq!(
        published_versions(&db, PACKAGE).await.expect("versions"),
        vec!["1.0.0".to_owned()]
    );
}

#[tokio::test]
async fn a_score_round_trips_without_a_numeric_decode_failure() {
    let db = connect().await;
    fixture(&db).await;

    // Unrated: no average rather than a zero.
    let empty = score(&db, PACKAGE).await.expect("score");
    assert_eq!(empty.average, None);
    assert_eq!(empty.count, 0);

    rate(&db, PSEUD, PACKAGE, "1.0.0", 4, Some("useful"))
        .await
        .expect("rate");
    let rated = score(&db, PACKAGE).await.expect("score");
    assert_eq!(rated.count, 1);
    assert_eq!(rated.average, Some(4.0));

    // `SUM(stars)` is NUMERIC on PostgreSQL: a decode that assumes INTEGER fails here and not on SQLite.
    let written = reviews(&db, PACKAGE, 10).await.expect("reviews");
    assert_eq!(written.len(), 1, "{written:?}");
    assert_eq!(written[0]["handle"], "gallery-pseud", "{written:?}");

    // Re-rating replaces rather than adding.
    rate(&db, PSEUD, PACKAGE, "1.0.0", 2, None)
        .await
        .expect("re-rate");
    let after = score(&db, PACKAGE).await.expect("score");
    assert_eq!(after.count, 1);
    assert_eq!(after.average, Some(2.0));
    // A rating with no text is not a review.
    assert!(reviews(&db, PACKAGE, 10).await.expect("reviews").is_empty());
}
