//! M69-02 — peer fingerprints through the store, on whichever engine `DARKHOLD_TEST_PG_URL` names.
//!
//! The claims, one assertion each:
//!
//! * a recorded document reads back as the same document (the peer's `enforced` layer survives — the veto depends on it);
//! * recording the same instance twice **replaces** rather than duplicating, because a fingerprint is a statement about
//!   now and two copies would leave the operator comparing against a stale one;
//! * a document that does not name itself is refused rather than filed under a guess;
//! * forgetting is a delete that reports whether anything was there.

use darkhold_db::{peer_fingerprints, Backend, Database};

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
            "darkhold-peerprints-{}-{}",
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
    let name = format!("lh_peer_{}", uuid::Uuid::new_v4().simple());
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

fn document(instance: &str, tags: &[(&str, f64)], enforced: &[&str]) -> String {
    let tags: Vec<serde_json::Value> = tags
        .iter()
        .map(|(tag, weight)| serde_json::json!({ "tag": tag, "weight": weight }))
        .collect();
    serde_json::json!({
        "version": 1,
        "instance_id": instance,
        "generated_at": "2026-11-01T00:00:00Z",
        "tags": tags,
        "enforced": enforced,
    })
    .to_string()
}

async fn row_count(db: &Database) -> i64 {
    match db.backend() {
        Backend::Sqlite => sqlx::query_scalar("SELECT COUNT(*) FROM peer_fingerprints")
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("count"),
        Backend::Postgres => sqlx::query_scalar("SELECT COUNT(*) FROM peer_fingerprints")
            .fetch_one(db.postgres_pool().expect("postgres"))
            .await
            .expect("count"),
    }
}

#[tokio::test]
async fn a_recorded_document_reads_back_with_the_layer_it_declared() {
    let db = connect().await;
    let db = &db;

    let stored = peer_fingerprints::record(
        db,
        &document(
            "https://peer.example",
            &[("angst", 0.6), ("fluff", 0.4)],
            &["csam"],
        ),
        "2026-11-07T00:00:00Z",
    )
    .await
    .expect("record");
    assert_eq!(stored.instance_id, "https://peer.example");
    assert!(stored.enforced.contains("csam"));

    let listed = peer_fingerprints::list(db).await.expect("list");
    assert_eq!(listed.len(), 1);
    let (fingerprint, fetched_at) = &listed[0];
    assert_eq!(fingerprint.instance_id, "https://peer.example");
    assert_eq!(fingerprint.tags.len(), 2);
    assert_eq!(fetched_at, "2026-11-07T00:00:00Z");
    assert!(
        fingerprint.enforced.contains("csam"),
        "the declared layer is what the veto compares, so it must survive the round trip"
    );

    // Recording again replaces, and the newer layer is the one kept.
    peer_fingerprints::record(
        db,
        &document(
            "https://peer.example",
            &[("angst", 0.5), ("smut", 0.5)],
            &[],
        ),
        "2026-11-08T00:00:00Z",
    )
    .await
    .expect("record again");
    assert_eq!(row_count(db).await, 1, "one peer, one document");
    let listed = peer_fingerprints::list(db).await.expect("list");
    assert!(
        listed[0].0.enforced.is_empty(),
        "the newer document is the one kept"
    );
    assert_eq!(listed[0].1, "2026-11-08T00:00:00Z");
}

#[tokio::test]
async fn an_anonymous_document_is_refused_and_forgetting_reports_whether_it_was_there() {
    let db = connect().await;
    let db = &db;

    let mut anonymous: serde_json::Value =
        serde_json::from_str(&document("https://x.example", &[("angst", 1.0)], &[])).expect("json");
    anonymous["instance_id"] = serde_json::json!("");
    let error = peer_fingerprints::record(db, &anonymous.to_string(), "2026-11-07T00:00:00Z")
        .await
        .expect_err("a document that does not name itself cannot be filed");
    assert!(
        error.to_string().contains("name its own instance"),
        "{error}"
    );

    assert!(!peer_fingerprints::forget(db, "https://never.example")
        .await
        .expect("forget"));

    peer_fingerprints::record(
        db,
        &document("https://gone.example", &[("angst", 1.0)], &["csam"]),
        "2026-11-07T00:00:00Z",
    )
    .await
    .expect("record");
    assert!(peer_fingerprints::forget(db, "https://gone.example")
        .await
        .expect("forget"));
    assert!(peer_fingerprints::list(db).await.expect("list").is_empty());
}
