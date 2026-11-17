//! M74-01 — the public syndicated catalogue shows metadata to everyone and body text only at TL2.
//!
//! The withheld case is a paired comparison, not a single `assert!(!contains("secret"))`: an empty
//! catalogue would pass that. Here a TL1 reader asks for a story that has a stored chapter and for
//! one that has no chapter; the two metadata records must have the same key set and neither may
//! carry a body-shaped key. A TL2 reader asking for the first story must get the stored text.

use axum::http::StatusCode;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::{imports, jobs, storage::BlobStore, syndication};
use darkhold_domain::{jobs::JobKind, jobs::RetryPolicy};
use serde_json::{json, Value};

fn router_for(db: &test_support::TestDb, root: &std::path::Path) -> axum::Router {
    let mut config = Config::development_defaults();
    config.storage.root = root.to_path_buf();
    config.syndication.account_email = "syndication-catalogue@darkhold.invalid".to_owned();
    server::build_router(AppState::new(config, db.db().clone()))
}

async fn client_at(
    db: &test_support::TestDb,
    router: &axum::Router,
    tag: &str,
    trust: i64,
) -> test_support::TestClient {
    let mut client = test_support::TestClient::new(router.clone());
    let handle = tag.replace(['.', '-'], "_");
    let account = test_support::register(&mut client, &format!("{tag}@test.dev"), &handle).await;
    darkhold_db::governance::set_trust(db.db(), &account, trust, "{}")
        .await
        .expect("set trust");
    client
}

async fn seed_story(
    db: &test_support::TestDb,
    root: &std::path::Path,
    account: darkhold_domain::AccountId,
    pseud: darkhold_domain::PseudId,
    key: &str,
    title: &str,
    body: Option<&str>,
) {
    let item = imports::upsert_library_item(
        db.db(),
        &account.to_string(),
        "pawchive",
        key,
        &imports::LibraryItemInput {
            title: title.to_owned(),
            author_text: "Vandalvagabond".to_owned(),
            author_url: Some("https://pawchive.pw/patreon/user/26677387".to_owned()),
            summary: "A story found through a recommended author.".to_owned(),
            language: Some("en".to_owned()),
            word_count: None,
            status: "unknown".to_owned(),
            source_url: "https://pawchive.pw/patreon/user/26677387".to_owned(),
            source_updated_at: None,
            provenance_json: json!({
                "source": "pawchive",
                "body_audience": "trust_at_least:2",
            })
            .to_string(),
        },
    )
    .await
    .expect("library item");

    if let Some(body) = body {
        let (checksum, _) = BlobStore::new(root.to_path_buf())
            .put(db.db(), body.as_bytes(), "text/html; charset=utf-8")
            .await
            .expect("stored body");
        let import_id = uuid::Uuid::new_v4().to_string();
        let queue = jobs::enqueue(
            db.db(),
            JobKind::Import,
            &json!({ "import_job_id": import_id }).to_string(),
            None,
            Some(account),
            0,
            &RetryPolicy::default(),
        )
        .await
        .expect("queue job");
        imports::create_import_job(
            db.db(),
            &import_id,
            &queue.to_string(),
            &account.to_string(),
            &pseud.to_string(),
            "pawchive",
            &item.source_url,
            "library",
            false,
        )
        .await
        .expect("import job");
        imports::upsert_import_unit(
            db.db(),
            &import_id,
            Some(&item.id),
            &imports::ImportUnitInput {
                source_unit_key: format!("{key}-chapter-1"),
                ordinal: 1,
                title: "Chapter 1".to_owned(),
                state: "stored".to_owned(),
                content_blob_checksum: Some(checksum),
                note: None,
            },
        )
        .await
        .expect("unit");
    }
}

fn item_by_key<'a>(body: &'a Value, key: &str) -> &'a Value {
    body["items"]
        .as_array()
        .expect("items array")
        .iter()
        .find(|item| item["source_work_key"] == key)
        .unwrap_or_else(|| panic!("catalogue has no {key}: {body}"))
}

fn key_set(value: &Value) -> Vec<&str> {
    let mut keys: Vec<&str> = value
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    keys
}

#[tokio::test]
async fn metadata_is_public_but_a_body_is_not_even_hinted_below_trust_two() {
    let root = std::env::temp_dir().join(format!("catalogue-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&root).expect("storage root");
    let db = test_support::TestDb::connect_with_dir("catalogue-visibility", &root).await;
    let account = syndication::ensure_account(db.db(), "syndication-catalogue@darkhold.invalid")
        .await
        .expect("instance account");
    let pseud = syndication::ensure_pseud(db.db(), account, "pawchive", "Pawchive")
        .await
        .expect("pseud");
    seed_story(
        &db,
        &root,
        account,
        pseud,
        "pawchive:1:with-body",
        "The Hidden Chapter",
        Some("SECRET BODY"),
    )
    .await;
    seed_story(
        &db,
        &root,
        account,
        pseud,
        "pawchive:1:no-body",
        "The Empty Record",
        None,
    )
    .await;
    let router = router_for(&db, &root);

    let mut anonymous = test_support::TestClient::new(router.clone());
    let mut tl1 = client_at(&db, &router, "tl1.catalogue", 1).await;
    let mut tl2 = client_at(&db, &router, "tl2.catalogue", 2).await;

    let (status, anon_body) = anonymous.get("/api/v1/catalogue/syndicated").await;
    assert_eq!(status, StatusCode::OK, "public metadata: {anon_body}");
    let (_, tl1_body) = tl1.get("/api/v1/catalogue/syndicated").await;
    let (status, tl2_body) = tl2.get("/api/v1/catalogue/syndicated").await;
    assert_eq!(status, StatusCode::OK, "TL2 body: {tl2_body}");

    for (name, body) in [("anonymous", &anon_body), ("TL1", &tl1_body)] {
        let with_body = item_by_key(body, "pawchive:1:with-body");
        let no_body = item_by_key(body, "pawchive:1:no-body");
        assert_eq!(
            with_body["title"], "The Hidden Chapter",
            "{name} sees metadata"
        );
        assert_eq!(
            no_body["title"], "The Empty Record",
            "{name} sees the other record"
        );
        assert_eq!(
            key_set(with_body),
            key_set(no_body),
            "{name} cannot tell the two apart by shape"
        );
        for forbidden in [
            "chapters",
            "body",
            "body_audience",
            "restricted",
            "locked",
            "unlock",
            "TL2",
        ] {
            assert!(
                !with_body
                    .as_object()
                    .expect("object")
                    .contains_key(forbidden),
                "{name} response leaked the {forbidden} key"
            );
        }
        let text = body.to_string();
        assert!(!text.contains("SECRET BODY"), "{name} saw the stored text");
    }

    let visible = item_by_key(&tl2_body, "pawchive:1:with-body");
    assert_eq!(
        visible["chapters"][0]["body"], "SECRET BODY",
        "TL2 sees the stored text"
    );
    db.db().close().await;
}
