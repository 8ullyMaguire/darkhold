//! Row 264 — reading back the body an import stored.
//!
//! The writer in `crates/app/src/imports.rs` puts each unit's bytes in the
//! existing `BlobStore`, registers a `library_item_unit` content reference and
//! records the checksum on the import unit. This file seeds exactly that shape
//! and then asks the new route for prose at a `(unit, offset)` anchor.
//!
//! The anchor is a chapter/section id plus a character offset inside *that*
//! unit. A byte offset into a fabricated merged document would make the address
//! depend on markup, so both the unit and an in-range offset are required.

use axum::http::StatusCode;
use darkhold_app::config::Config;
use darkhold_app::imports::UNIT_OWNER_TYPE;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::storage::BlobStore;
use darkhold_db::{imports, jobs};
use darkhold_domain::jobs::{JobKind, RetryPolicy};
use serde_json::json;

const BODY_A: &str = "<p>ALPHA BODY</p>";
const BODY_B: &str = "<p>BETA BODY</p>";

struct Harness {
    _tdb: test_support::TestDb,
    _root: std::path::PathBuf,
    router: axum::Router,
}

impl Harness {
    async fn new(tag: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("dh264-{tag}-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&root).expect("storage root");
        let tdb = test_support::TestDb::connect_with_dir(tag, &root).await;
        let mut config = Config::development_defaults();
        config.storage.root = root.clone();
        let router = server::build_router(AppState::new(config, tdb.db().clone()));
        Self {
            _tdb: tdb,
            _root: root,
            router,
        }
    }

    fn client(&self) -> test_support::TestClient {
        test_support::TestClient::new(self.router.clone())
    }

    async fn reader(&self, handle: &str) -> (test_support::TestClient, String) {
        let mut client = self.client();
        let account =
            test_support::register(&mut client, &format!("{handle}@example.test"), handle).await;
        (client, account)
    }
}

async fn pseud_of(db: &darkhold_db::Database, account: &str) -> String {
    let sql = db.sql(
        "SELECT id FROM pseuds WHERE account_id = ? ORDER BY created_at LIMIT 1",
        "SELECT id::text FROM pseuds WHERE account_id::text = $1 ORDER BY created_at LIMIT 1",
    );
    match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar::<_, String>(&sql)
            .bind(account)
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("pseud"),
        darkhold_db::Backend::Postgres => sqlx::query_scalar::<_, String>(&sql)
            .bind(account)
            .fetch_one(db.postgres_pool().expect("postgres"))
            .await
            .expect("pseud"),
    }
}

/// Store through the importer's writers: blob, content reference, item and units.
async fn seed_item(h: &Harness, account: &str, key: &str) -> String {
    let db = h._tdb.db();
    let account_id: darkhold_domain::AccountId = account.parse().expect("account id");
    let pseud = pseud_of(db, account).await;
    let item = imports::upsert_library_item(
        db,
        account,
        "import-fixture",
        key,
        &imports::LibraryItemInput {
            title: "A private imported work".to_owned(),
            author_text: "Nobody".to_owned(),
            author_url: None,
            summary: String::new(),
            language: Some("en".to_owned()),
            word_count: None,
            status: "unknown".to_owned(),
            source_url: "https://example.test/story".to_owned(),
            source_updated_at: None,
            provenance_json: "{}".to_owned(),
        },
    )
    .await
    .expect("library item");

    let import_id = uuid::Uuid::new_v4().to_string();
    let queue = jobs::enqueue(
        db,
        JobKind::Import,
        &json!({ "import_job_id": import_id }).to_string(),
        None,
        Some(account_id),
        0,
        &RetryPolicy::default(),
    )
    .await
    .expect("queue job");
    imports::create_import_job(
        db,
        &import_id,
        &queue.to_string(),
        account,
        &pseud,
        "import-fixture",
        &item.source_url,
        "library",
        false,
    )
    .await
    .expect("import job");

    let store = BlobStore::new(h._root.clone());
    for (ordinal, (unit_key, body)) in [("ch-1", BODY_A), ("ch-2", BODY_B)].into_iter().enumerate()
    {
        // Same sequence as the writer: put -> reference -> upsert unit.
        let (checksum, _) = store
            .put(db, body.as_bytes(), "text/html")
            .await
            .expect("stored body");
        store
            .reference(db, &checksum, UNIT_OWNER_TYPE, &item.id)
            .await
            .expect("content reference");
        imports::upsert_import_unit(
            db,
            &import_id,
            Some(&item.id),
            &imports::ImportUnitInput {
                source_unit_key: unit_key.to_owned(),
                ordinal: i64::from(ordinal as u32) + 1,
                title: format!("Section {unit_key}"),
                state: "stored".to_owned(),
                content_blob_checksum: Some(checksum),
                note: None,
            },
        )
        .await
        .expect("import unit");
    }
    item.id
}

fn path(item: &str, unit: &str, offset: usize) -> String {
    format!("/api/v1/library/items/{item}/body?unit={unit}&offset={offset}")
}

#[tokio::test]
async fn a_stored_body_is_read_back_at_its_own_anchor() {
    let h = Harness::new("body-roundtrip").await;
    let (mut owner, account) = h.reader("owner").await;
    let item = seed_item(&h, &account, "body-roundtrip:1").await;

    let (status, body) = owner.get(path(&item, "ch-2", 6)).await;
    assert_eq!(status, StatusCode::OK, "read stored body: {body}");
    assert_eq!(body["unit"], "ch-2");
    assert_eq!(body["ordinal"], 2);
    assert_eq!(body["offset"], 6);
    assert_eq!(
        body["body"],
        &BODY_B[6..],
        "offset is inside the anchored unit"
    );
    assert_eq!(body["length"], BODY_B.chars().count());
    assert!(
        body.to_string().contains("A BODY"),
        "the stored words came back: {body}"
    );

    let (status, first) = owner.get(path(&item, "ch-1", 0)).await;
    assert_eq!(status, StatusCode::OK, "first stored unit: {first}");
    assert_eq!(first["body"], BODY_A);
}

#[tokio::test]
async fn an_anchor_is_a_unit_plus_an_offset_not_a_byte_into_a_merged_document() {
    let h = Harness::new("anchor-rule").await;
    let (mut owner, account) = h.reader("anchor-owner").await;
    let item = seed_item(&h, &account, "anchor-rule:1").await;

    // A byte offset past the first unit's own end must be refused, even though
    // that byte exists in the concatenation of the two stored bodies.
    let merged_offset = BODY_A.len() + 6;
    let (status, body) = owner.get(path(&item, "ch-1", merged_offset)).await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "merged offset refused: {body}"
    );

    let (status, body) = owner
        .get(path(&item, "ch-2", BODY_B.chars().count() + 1))
        .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "past-end offset refused: {body}"
    );

    let (status, body) = owner.get(path(&item, "not-a-section", 0)).await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "unknown unit refused: {body}"
    );

    // Both halves are mandatory: axum refuses the query before the handler.
    let (status, _) = owner
        .get(format!("/api/v1/library/items/{item}/body?offset=0"))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "missing unit refused");
    let (status, _) = owner
        .get(format!("/api/v1/library/items/{item}/body?unit=ch-1"))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "missing offset refused");
}

#[tokio::test]
async fn a_stranger_cannot_read_a_private_items_stored_body() {
    let h = Harness::new("stranger-refusal").await;
    let (mut owner, account) = h.reader("body-owner").await;
    let item = seed_item(&h, &account, "stranger-refusal:1").await;
    let url = path(&item, "ch-1", 0);

    let mut anonymous = h.client();
    let (status, body) = anonymous.get(&url).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "visitor refused: {body}");
    assert!(!body.to_string().contains("ALPHA BODY"));

    let (mut stranger, _) = h.reader("body-stranger").await;
    let (status, body) = stranger.get(&url).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "another reader refused: {body}"
    );
    assert!(!body.to_string().contains("ALPHA BODY"));

    let (status, body) = owner.get(&url).await;
    assert_eq!(status, StatusCode::OK, "owner still reads: {body}");
    assert_eq!(body["body"], json!(BODY_A));
}
