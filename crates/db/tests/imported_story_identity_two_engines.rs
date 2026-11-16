//! M74-01: two chapter posts from one Pawchive story must become one library item with two units.
//!
//! Runs on whichever engine `DARKHOLD_TEST_PG_URL` names: SQLite when unset, PostgreSQL when set.

use darkhold_db::{imports, jobs, syndication, Backend, Database, DatabaseConfig};
use darkhold_domain::AccountId;

fn config(url: String) -> DatabaseConfig {
    let mut c = DatabaseConfig::new(url);
    c.max_connections = 2;
    c.acquire_timeout = std::time::Duration::from_secs(60);
    c
}

async fn connect() -> Database {
    let Ok(admin) = std::env::var("DARKHOLD_TEST_PG_URL") else {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "darkhold-story-identity-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let db = Database::connect(&config(format!(
            "sqlite://{}/darkhold.sqlite?mode=rwc",
            dir.display()
        )))
        .await
        .expect("connect sqlite");
        db.migrate().await.expect("migrate");
        return db;
    };
    let name = format!("lh_story_{}", uuid::Uuid::new_v4().simple());
    let admin_db = Database::connect(&config(admin.clone()))
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
    let db = Database::connect(&config(url)).await.expect("connect");
    db.migrate().await.expect("migrate");
    db
}

#[tokio::test]
async fn chapters_of_one_story_share_a_library_item_on_both_engines() {
    let db = connect().await;
    let account: AccountId = syndication::ensure_account(
        &db,
        &format!("m74-{}@darkhold.invalid", uuid::Uuid::new_v4().simple()),
    )
    .await
    .expect("instance account");
    let pseud = syndication::ensure_pseud(&db, account, "pawchive", "Pawchive")
        .await
        .expect("source pseud");

    let key = "pawchive:26677387:sailing-the-skies-with-my-endergirl";
    let import_id = uuid::Uuid::new_v4().to_string();
    let queue = jobs::enqueue(
        &db,
        darkhold_domain::jobs::JobKind::Import,
        &serde_json::json!({ "import_job_id": import_id }).to_string(),
        None,
        Some(account),
        0,
        &darkhold_domain::jobs::RetryPolicy::default(),
    )
    .await
    .expect("queue");
    imports::create_import_job(
        &db,
        &import_id,
        &queue.to_string(),
        &account.to_string(),
        &pseud.to_string(),
        "pawchive",
        "https://pawchive.pw/patreon/user/26677387",
        "library",
        false,
    )
    .await
    .expect("import job");

    for (index, (post_id, title)) in [
        ("post-10", "Sailing the Skies with my Endergirl 10"),
        (
            "post-11",
            "Sailing the Skies with my Endergirl - Chapter 11",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let item = imports::upsert_library_item(
            &db,
            &account.to_string(),
            "pawchive",
            key,
            &imports::LibraryItemInput {
                title: title.to_owned(),
                author_text: "Vandalvagabond".to_owned(),
                author_url: Some("https://pawchive.pw/patreon/user/26677387".to_owned()),
                summary: "One story, many chapter posts.".to_owned(),
                language: Some("en".to_owned()),
                word_count: None,
                status: "unknown".to_owned(),
                source_url: "https://pawchive.pw/patreon/user/26677387".to_owned(),
                source_updated_at: None,
                provenance_json: serde_json::json!({
                    "source": "pawchive",
                    "body_audience": "trust_at_least:2",
                })
                .to_string(),
            },
        )
        .await
        .expect("library item");
        imports::upsert_import_unit(
            &db,
            &import_id,
            Some(&item.id),
            &imports::ImportUnitInput {
                source_unit_key: post_id.to_owned(),
                ordinal: index as i64 + 1,
                title: title.to_owned(),
                state: "stored".to_owned(),
                content_blob_checksum: None,
                note: None,
            },
        )
        .await
        .expect("unit");
        imports::set_import_state(&db, &import_id, "completed", Some(&item.id), None)
            .await
            .expect("settle");
    }

    let items = imports::list_library_items(&db, &account.to_string(), 50, None)
        .await
        .expect("list");
    assert_eq!(items.len(), 1, "both posts belong to one story");
    assert_eq!(items[0].source_work_key, key);
    assert_eq!(
        items[0].title,
        "Sailing the Skies with my Endergirl - Chapter 11"
    );
    let units = imports::latest_units_for_item(&db, &items[0].id)
        .await
        .expect("units");
    assert_eq!(units.len(), 2, "the chapters are units, not works");
    assert!(matches!(db.backend(), Backend::Sqlite | Backend::Postgres));
}
