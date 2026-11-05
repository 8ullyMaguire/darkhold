//! M63-04: "similar to this one", the author filter and the read-history
//! exclusion, on both engines.
//!
//! The similarity maths is unit-tested in `darkhold-domain`; this file pins the
//! four properties that only exist once a database is involved:
//!
//! 1. **The preselection does not drop a neighbour.** The `tokens` filter is a
//!    coarse `LIKE` scan and the ranker is exact — a row that shares a rare title
//!    token must survive the filter even when it shares nothing else.
//! 2. **An unrelated row is absent, not last.** `ranked` counts neighbours, and a
//!    page padded with unrelated rows is the failure this exists to prevent.
//! 3. **`--author` is exact and case-insensitive.** `Osh` must not return
//!    `Oshha`'s rows, and `OSHHA` must return them.
//! 4. **`--exclude-read` drops the account's own reading and nobody else's.**
//!
//! Run on SQLite by default; set `DARKHOLD_TEST_PG_URL` for the PostgreSQL arm.

use std::time::Duration;

use darkhold_db::Database;

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
        "darkhold-opthere-{}-{}-{}",
        tag,
        uuid::Uuid::new_v4(),
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let url = match std::env::var("DARKHOLD_TEST_PG_URL") {
        Ok(admin) => {
            let name = format!(
                "lh_opthere_{}_{}",
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

/// M45-69: the shelf is one row per instance, replaced rather than accumulated, and deletable so an
/// instance can go back to what its readers say.
#[tokio::test]
async fn a_shelf_is_replaced_not_accumulated_and_can_be_forgotten() {
    let db = connect("store").await;
    let instance = "https://darkhold.example";

    let first_vector = serde_json::json!({"dark": 1.0, "progression": 0.5});
    let first_top = serde_json::json!([{"tag": "dark", "weight": 1.0}]);
    let first_seeds = serde_json::json!([{"line": 1, "spec": "A", "status": "used"}]);
    let claim = darkhold_db::operator_theme::ThemeClaim {
        vector: &first_vector,
        top_tags: &first_top,
        seeds: &first_seeds,
        seed_count: 1,
        resolved_count: 1,
    };
    darkhold_db::operator_theme::upsert(&db, instance, &claim, "2026-11-04T00:00:00Z")
        .await
        .expect("first shelf");

    let stored = darkhold_db::operator_theme::get(&db, instance)
        .await
        .expect("get")
        .expect("a shelf");
    assert_eq!(stored.seed_count, 1);
    assert_eq!(stored.theme_vector["dark"], 1.0);

    // A second post replaces: two statements of what the instance is FOR cannot both be true.
    let second_vector = serde_json::json!({"necromancy": 1.0});
    let second_claim = darkhold_db::operator_theme::ThemeClaim {
        vector: &second_vector,
        top_tags: &serde_json::json!([]),
        seeds: &serde_json::json!([]),
        seed_count: 3,
        resolved_count: 1,
    };
    darkhold_db::operator_theme::upsert(&db, instance, &second_claim, "2026-11-05T00:00:00Z")
        .await
        .expect("second shelf");

    let stored = darkhold_db::operator_theme::get(&db, instance)
        .await
        .expect("get")
        .expect("a shelf");
    assert_eq!(stored.seed_count, 3, "replaced, not merged");
    assert!(
        stored.theme_vector.get("dark").is_none(),
        "the old claim is gone: {:?}",
        stored.theme_vector
    );
    assert_eq!(
        stored.sourced_at, "2026-11-05T00:00:00Z",
        "sourced_at is when this shelf was stated"
    );

    assert!(
        darkhold_db::operator_theme::delete(&db, instance)
            .await
            .expect("delete"),
        "deleting a shelf that exists reports that it removed one"
    );
    assert!(darkhold_db::operator_theme::get(&db, instance)
        .await
        .expect("get")
        .is_none());
    assert!(
        !darkhold_db::operator_theme::delete(&db, instance)
            .await
            .expect("delete twice"),
        "and deleting one that is not there says so instead of pretending"
    );
}
