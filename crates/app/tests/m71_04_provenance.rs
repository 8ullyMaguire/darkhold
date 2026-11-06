//! M71-04 — every list payload that names a work says where its text lives.
//!
//! The catalogue has always been two pools, and the words for them are the API's: `hosted`, `hosted_locked`,
//! `referenced`, `known`. A badge coined in the interface would be a second vocabulary that drifts.
//!
//! What this asserts:
//!
//! 1. the tag browse payload carries `provenance`, `hosted` for a work with a readable unit and `referenced` for one
//!    whose only trace is a catalogue row with a source URL;
//! 2. a work with neither is `known`, and the word is not omitted for it either;
//! 3. the document surfaces use the same vocabulary: the work page says the text is here, the tag page marks the
//!    pointer as "elsewhere";
//! 4. the vocabulary has exactly four words, in one place (`routes::provenance`), tested in that module.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server::{self, set_trust_proxy};
use darkhold_app::state::AppState;
use darkhold_db::DatabaseConfig;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tower::ServiceExt;

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-prov-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn config_for(dir: &Path) -> Config {
    let mut config = Config::development_defaults();
    config.storage.root = dir.to_path_buf();
    config.database = DatabaseConfig::new(format!(
        "sqlite://{}/darkhold.sqlite?mode=rwc",
        dir.display()
    ));
    config
}

const ACCOUNT: &str = "01955000-0000-4000-8000-0000000002a1";
const PSEUD: &str = "01955000-0000-4000-8000-0000000002e1";
const HOSTED: &str = "01957000-0000-4000-8000-0000000002b1";
const REFERENCED: &str = "01957000-0000-4000-8000-0000000002b2";
const KNOWN: &str = "01957000-0000-4000-8000-0000000002b3";
const UNIT: &str = "01956000-0000-4000-8000-0000000002c1";
const REVISION: &str = "01956000-0000-4000-8000-0000000002c2";
const TAG: &str = "01954000-0000-4000-8000-0000000002f1";
const ITEM: &str = "01958000-0000-4000-8000-0000000002d1";

struct Harness {
    _dir: PathBuf,
    tdb: test_support::TestDb,
    config: Config,
}

impl Harness {
    async fn new(tag: &str) -> Self {
        set_trust_proxy(false);
        let dir = scratch_dir(tag);
        let tdb = test_support::TestDb::connect_with_dir(tag, &dir).await;
        let config = config_for(&dir);
        Self {
            _dir: dir,
            tdb,
            config,
        }
    }

    fn client(&self) -> axum::Router {
        server::build_router(AppState::new(
            self.config.clone(),
            self.tdb.db().clone(),
        ))
    }

    async fn exec(&self, statement: &str) {
        sqlx::query(statement)
            .execute(self.tdb.db().sqlite_pool().expect("sqlite"))
            .await
            .unwrap_or_else(|error| panic!("{statement}\n{error}"));
    }

    async fn seed(&self) {
        self.exec(&format!(
            "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version)
             VALUES ('{ACCOUNT}','active','prov@example.invalid','adult',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ))
        .await;
        self.exec(&format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at,
                                 updated_at, version)
             VALUES ('{PSEUD}','{ACCOUNT}','provwriter','Prov Writer','listed',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ))
        .await;
        self.exec(&format!(
            "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at)
             VALUES ('{TAG}','tag','two pools','two pools','2026-01-01T00:00:00Z')"
        ))
        .await;

        for (id, title) in [
            (HOSTED, "Held Here"),
            (REFERENCED, "Pointed At"),
            (KNOWN, "Merely Known"),
        ] {
            self.exec(&format!(
                "INSERT INTO works (id, owner_pseud_id, title, language, rating, visibility, lifecycle,
                                    created_at, updated_at, version)
                 VALUES ('{id}','{PSEUD}','{title}','en','general','public','published',
                         '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
            ))
            .await;
            self.exec(&format!(
                "INSERT INTO work_tags (work_id, node_id, prominence, added_at)
                 VALUES ('{id}','{TAG}',1,'2026-01-01T00:00:00Z')"
            ))
            .await;
        }

        // The hosted work has a readable unit; the referenced one has a catalogue row with a source URL; the third has
        // neither, which is the `known` case and the one a three-word vocabulary would have conflated.
        self.exec(&format!(
            "INSERT INTO units (id, work_id, order_key, title, current_revision_id, created_at,
                                updated_at, version)
             VALUES ('{UNIT}','{HOSTED}',10,'One',NULL,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ))
        .await;
        self.exec(&format!(
            "INSERT INTO revisions (id, unit_id, revision_number, document_json, sanitized_html,
                                    plain_text, word_count, created_by_pseud_id, created_at)
             VALUES ('{REVISION}','{UNIT}',1,'{{}}','<p>Text.</p>','Text.',10,'{PSEUD}',
                     '2026-01-01T00:00:00Z')"
        ))
        .await;
        self.exec(&format!("UPDATE units SET current_revision_id = '{REVISION}' WHERE id = '{UNIT}'"))
            .await;

        self.exec(&format!(
            "INSERT INTO library_items (id, account_id, work_id, source_key, source_work_key, title,
                                        author_text, summary, status, source_url, provenance_json,
                                        created_at, updated_at, version)
             VALUES ('{ITEM}','{ACCOUNT}','{REFERENCED}','spacebattles','thread-1','Pointed At',
                     'someone','',  'unknown','https://example.invalid/thread-1','{{}}',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ))
        .await;
    }
}

async fn get_json(app: axum::Router, uri: &str) -> (StatusCode, Value) {
    let response = app
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let value = serde_json::from_slice(&body)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&body).into_owned()));
    (status, value)
}

async fn get_html(app: axum::Router, uri: &str) -> (StatusCode, String) {
    let response = app
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&body).into_owned())
}

#[tokio::test]
async fn the_tag_payload_names_where_each_works_text_lives() {
    let harness = Harness::new("tag-payload").await;
    harness.seed().await;
    let (status, body) = get_json(harness.client(), "/api/v1/tags/two%20pools").await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let items = body["items"].as_array().expect("items");
    assert_eq!(items.len(), 3, "{body}");

    let word = |id: &str| {
        items
            .iter()
            .find(|item| item["work_id"] == id)
            .and_then(|item| item["provenance"].as_str())
            .unwrap_or("<missing>")
            .to_owned()
    };
    assert_eq!(word(HOSTED), "hosted", "{body}");
    assert_eq!(word(REFERENCED), "referenced", "{body}");
    assert_eq!(word(KNOWN), "known", "{body}");
}

#[tokio::test]
async fn the_work_page_says_the_text_is_here() {
    let harness = Harness::new("work-page").await;
    harness.seed().await;
    let (status, body) = get_html(harness.client(), &format!("/works/{HOSTED}")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.contains("The text is here."),
        "a hosted work says so in the same vocabulary: {body}"
    );
}

#[tokio::test]
async fn the_tag_page_marks_a_pointer_as_elsewhere() {
    let harness = Harness::new("tag-page").await;
    harness.seed().await;
    let (status, body) = get_html(harness.client(), "/tags/two%20pools").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("readable here"), "{body}");
    assert!(
        body.contains("elsewhere"),
        "the pointer is not offered as if it were a chapter: {body}"
    );
}
