//! The webhook-accounting queries on whichever engine `DARKHOLD_TEST_PG_URL` names.
//!
//! Rotation writes two columns from one read; the count excludes throttled rows; the breaker writes the
//! switch and its reason in one statement. Each of those is one statement per engine, so the only way to
//! know both dialects agree is to run them both.

use darkhold_db::Database;

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
            "darkhold-webhooks-2e-{}-{}",
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
    let name = format!("lh_wh_{}", uuid::Uuid::new_v4().simple());
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

#[tokio::test]
async fn rotation_counting_and_the_breaker_agree_on_both_engines() {
    let db = connect().await;
    let db = &db;

    let owner = "11111111-2222-3333-4444-000000000001";
    darkhold_db::marketplace::create_webhook(
        db,
        owner,
        "https://example.com/hook",
        "whsec_original",
        &["work.published".to_string()],
    )
    .await
    .expect("create webhook");

    // The id `create_webhook` chose is not ours to pass; read it back for the rest of the test.
    let rows = darkhold_db::marketplace::list_all_active_webhooks(db)
        .await
        .expect("list");
    let row = rows
        .iter()
        .find(|r| r["owner"] == serde_json::json!(owner))
        .expect("endpoint row");
    let id = row["id"].as_str().expect("id").to_owned();

    // --- rotation
    let rotated = darkhold_db::webhooks::rotate_secret(
        db,
        &id,
        owner,
        "whsec_second",
        "2026-11-07T00:00:00Z",
    )
    .await
    .expect("rotate");
    assert_eq!(rotated.as_deref(), Some("whsec_second"));

    let rows = darkhold_db::marketplace::list_all_active_webhooks(db)
        .await
        .expect("list");
    let row = rows.iter().find(|r| r["id"] == serde_json::json!(id)).expect("row");
    assert_eq!(row["secret"], serde_json::json!("whsec_second"));
    assert_eq!(row["previous_secret"], serde_json::json!("whsec_original"));
    assert_eq!(row["rotated_at"], serde_json::json!("2026-11-07T00:00:00Z"));

    // Somebody else's rotation is no rotation.
    let refused = darkhold_db::webhooks::rotate_secret(
        db,
        &id,
        "99999999-2222-3333-4444-000000000009",
        "whsec_stolen",
        "2026-11-07T00:00:00Z",
    )
    .await
    .expect("rotate");
    assert!(refused.is_none(), "a stranger must not rotate the secret");

    // --- the count
    for n in 0..2 {
        darkhold_db::marketplace::record_delivery(
            db,
            &id,
            &format!("evt-{n}"),
            "{}",
            "sig",
            "ok",
        )
        .await
        .expect("record delivery");
    }
    darkhold_db::marketplace::record_delivery(db, &id, "evt-t", "{}", "", "throttled")
        .await
        .expect("record throttled");
    let counted = darkhold_db::webhooks::delivered_since(db, &id, "2000-01-01T00:00:00Z")
        .await
        .expect("count");
    assert_eq!(counted, 2, "throttled rows never reached the receiver");

    // --- the breaker
    let mut switched = None;
    for _ in 0..3 {
        switched = darkhold_db::webhooks::record_failure(db, &id, 3, "2026-11-07T01:00:00Z")
            .await
            .expect("record failure");
    }
    let reason = switched.expect("the third failure switches it off");
    assert!(reason.contains("consecutive"), "{reason}");

    let subs = darkhold_db::webhooks::subscriptions(db).await.expect("subscriptions");
    let row = subs.iter().find(|r| r["id"] == serde_json::json!(id)).expect("row");
    assert_eq!(row["active"], serde_json::json!(false));
    assert_eq!(row["disabled_reason"], serde_json::json!(reason));
    let active = darkhold_db::marketplace::list_all_active_webhooks(db)
        .await
        .expect("active");
    assert!(!active.iter().any(|r| r["id"] == serde_json::json!(id)));

    // --- explicit enable, owner-scoped
    assert!(!darkhold_db::webhooks::reenable(db, &id, Some("someone-else"))
        .await
        .expect("reenable"));
    assert!(darkhold_db::webhooks::reenable(db, &id, Some(owner))
        .await
        .expect("reenable"));
    let subs = darkhold_db::webhooks::subscriptions(db).await.expect("subscriptions");
    let row = subs.iter().find(|r| r["id"] == serde_json::json!(id)).expect("row");
    assert_eq!(row["active"], serde_json::json!(true));
    assert_eq!(row["consecutive_failures"], serde_json::json!(0));
    assert_eq!(row["disabled_reason"], serde_json::Value::Null);
}
