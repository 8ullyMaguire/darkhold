//! M69-06 — the rescue ring's store, on whichever engine `DARKHOLD_TEST_PG_URL` names.
//!
//! The claims that are the store's rather than the domain's:
//!
//! * a hunt is recorded **whatever it found**, `not_found` included — "we looked and nobody had it" is the answer an
//!   operator needs most;
//! * an ambiguous hunt stores **no** matched instance, so the ring cannot be read as having chosen;
//! * accepting needs a name, and a second settle is a no-op rather than a silent overwrite;
//! * a survivor's copy is keyed by (instance, content hash), so re-announcing updates instead of duplicating.

use darkhold_db::{media_rescue, Database};
use darkhold_domain::media_rescue::Wanted;

const NOW: &str = "2026-11-07T00:00:00Z";

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
            "darkhold-rescue-{}-{}",
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
    let name = format!("lh_rescue_{}", uuid::Uuid::new_v4().simple());
    let admin_db = Database::connect(&make_config(admin.clone())).await.expect("admin");
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

fn wanted(content: &str, perceptual: Option<&str>) -> Wanted {
    Wanted {
        content_hash: content.to_owned(),
        perceptual_hash: perceptual.map(str::to_owned),
        width: Some(400),
        height: Some(300),
    }
}

#[tokio::test]
async fn an_exact_copy_is_found_and_an_ambiguous_one_names_nobody() {
    let db = connect().await;
    let db = &db;

    media_rescue::record_copy(
        db,
        &media_rescue::CopyClaim {
            instance: "https://alive.example",
            content_hash: "content-alive-0001",
            perceptual_hash: None,
            width: Some(400),
            height: Some(300),
            media_kind: "image",
            now: NOW,
        },
    )
    .await
    .expect("copy");

    let hunt = media_rescue::request(
        db,
        "https://dead.example",
        "media-1",
        &wanted("content-alive-0001", None),
        10,
        NOW,
    )
    .await
    .expect("hunt");
    assert_eq!(hunt.outcome, "exact");
    assert_eq!(hunt.matched_instance.as_deref(), Some("https://alive.example"));
    assert_eq!(hunt.threshold, 10, "the bound the search used travels with the outcome");

    // Two survivors with the same bytes: ambiguity, and no name stored.
    media_rescue::record_copy(
        db,
        &media_rescue::CopyClaim {
            instance: "https://second.example",
            content_hash: "content-alive-0001",
            perceptual_hash: None,
            width: Some(400),
            height: Some(300),
            media_kind: "image",
            now: NOW,
        },
    )
    .await
    .expect("second copy");
    let hunt = media_rescue::request(
        db,
        "https://dead.example",
        "media-2",
        &wanted("content-alive-0001", None),
        10,
        NOW,
    )
    .await
    .expect("hunt again");
    assert_eq!(hunt.outcome, "ambiguous");
    assert_eq!(
        hunt.matched_instance, None,
        "an ambiguous hunt must not read as a choice"
    );
    let candidates: serde_json::Value =
        serde_json::from_str(&hunt.candidates_json).expect("candidates");
    assert_eq!(candidates.as_array().expect("a list").len(), 2);

    // Nothing found is still a row.
    let hunt = media_rescue::request(
        db,
        "https://dead.example",
        "media-3",
        &wanted("content-missing-9999", None),
        10,
        NOW,
    )
    .await
    .expect("hunt");
    assert_eq!(hunt.outcome, "not_found");
    assert_eq!(hunt.matched_instance, None);
    let all = media_rescue::list(db, 50).await.expect("list");
    assert_eq!(all.len(), 3, "every hunt is recorded, including the empty ones");
    assert_eq!(all.iter().filter(|hunt| hunt.resolved_at.is_none()).count(), 3);
}

#[tokio::test]
async fn accepting_needs_a_name_and_settling_twice_is_a_no_op() {
    let db = connect().await;
    let db = &db;

    media_rescue::record_copy(
        db,
        &media_rescue::CopyClaim {
            instance: "https://a.example",
            content_hash: "content-tie-000001",
            perceptual_hash: Some("ffff0000ffff0000"),
            width: Some(400),
            height: Some(300),
            media_kind: "image",
            now: NOW,
        },
    )
    .await
    .expect("copy a");
    media_rescue::record_copy(
        db,
        &media_rescue::CopyClaim {
            instance: "https://b.example",
            content_hash: "content-tie-000002",
            perceptual_hash: Some("ffff0000ffff0000"),
            width: Some(400),
            height: Some(300),
            media_kind: "image",
            now: NOW,
        },
    )
    .await
    .expect("copy b");

    let hunt = media_rescue::request(
        db,
        "https://dead.example",
        "media-tie",
        &wanted("content-gone-0000", Some("ffff0000ffff0000")),
        4,
        NOW,
    )
    .await
    .expect("hunt");
    assert_eq!(hunt.outcome, "ambiguous", "{hunt:?}");

    // An unknown resolution is refused, and accepting without a name on an ambiguous hunt is refused.
    assert!(media_rescue::resolve(db, &hunt.id, "maybe", "operator", None, NOW)
        .await
        .is_err());
    let error = media_rescue::resolve(db, &hunt.id, "accepted", "operator", None, NOW)
        .await
        .expect_err("an ambiguous hunt needs the name the operator chose");
    assert!(error.to_string().contains("survivor"), "{error}");

    // Naming one settles it, and the name is stored as the credit.
    assert!(
        media_rescue::resolve(db, &hunt.id, "accepted", "operator", Some("https://b.example"), NOW)
            .await
            .expect("accept")
    );
    let settled = media_rescue::find(db, &hunt.id).await.expect("find").expect("row");
    assert_eq!(settled.resolution.as_deref(), Some("accepted"));
    assert_eq!(settled.matched_instance.as_deref(), Some("https://b.example"));
    assert_eq!(settled.resolved_by.as_deref(), Some("operator"));

    // A second settle changes nothing: the first decision is the one that happened.
    assert!(!media_rescue::resolve(db, &hunt.id, "rejected", "someone-else", None, "2026-11-08T00:00:00Z")
        .await
        .expect("settle again"));
    let settled = media_rescue::find(db, &hunt.id).await.expect("find").expect("row");
    assert_eq!(settled.resolution.as_deref(), Some("accepted"));
    assert_eq!(settled.resolved_at.as_deref(), Some(NOW));
}

#[tokio::test]
async fn a_re_announced_copy_updates_rather_than_duplicates() {
    let db = connect().await;
    let db = &db;
    let first = media_rescue::record_copy(
        db,
        &media_rescue::CopyClaim {
            instance: "https://alive.example",
            content_hash: "content-again-0001",
            perceptual_hash: None,
            width: None,
            height: None,
            media_kind: "image",
            now: NOW,
        },
    )
    .await
    .expect("copy");
    let again = media_rescue::record_copy(
        db,
        &media_rescue::CopyClaim {
            instance: "https://alive.example",
            content_hash: "content-again-0001",
            perceptual_hash: Some("ffff0000ffff0000"),
            width: Some(400),
            height: Some(300),
            media_kind: "image",
            now: "2026-11-08T00:00:00Z",
        },
    )
    .await
    .expect("copy again");
    assert_eq!(again.id, first.id, "one claim per (instance, bytes)");
    assert_eq!(
        again.perceptual_hash.as_deref(),
        Some("ffff0000ffff0000"),
        "a later announcement with a hash is better information, not a second copy"
    );
    assert_eq!(again.width, Some(400));
    assert_eq!(
        media_rescue::list_copies(db, 10).await.expect("list").len(),
        1
    );
}
