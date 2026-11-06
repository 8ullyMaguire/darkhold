//! ADR 0006 / M71-20 — a tag's page is a document.
//!
//! Four assertions:
//!
//! 1. a tag lists the works behind it as links to the work page, with **no script element**;
//! 2. the listing is what a reader may reach — a private work tagged with the same word does not appear, because the
//!    query filters to published public works and the page does not add a second, weaker filter of its own;
//! 3. a tag with nothing behind it is a 404 page, not an empty list (§8.11);
//! 4. the path decodes: `/tags/slow%20burn` finds the tag a link wrote as `slow burn`.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server::{self, set_trust_proxy};
use darkhold_app::state::AppState;
use darkhold_db::DatabaseConfig;
use std::path::{Path, PathBuf};
use tower::ServiceExt;

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-tagpage-{tag}-{}-{:?}",
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

const PUBLIC_A: &str = "01957000-0000-4000-8000-0000000001a1";
const PUBLIC_B: &str = "01957000-0000-4000-8000-0000000001b1";
const PRIVATE_C: &str = "01957000-0000-4000-8000-0000000001c1";
const PSEUD: &str = "01955000-0000-4000-8000-0000000001e1";
const ACCOUNT: &str = "01955000-0000-4000-8000-0000000001a1";
const TAG: &str = "01954000-0000-4000-8000-0000000001f1";

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

    async fn work(&self, id: &str, title: &str, visibility: &str, tagged: bool) {
        self.exec(&format!(
            "INSERT INTO works (id, owner_pseud_id, title, language, rating, visibility, lifecycle,
                                created_at, updated_at, version)
             VALUES ('{id}','{PSEUD}',{title},'en','general','{visibility}','published',
                     '2026-01-01T00:00:00Z','2026-01-0{}T00:00:00Z',1)",
            if tagged { 3 } else { 2 }
        ))
        .await;
        if tagged {
            self.exec(&format!(
                "INSERT INTO work_tags (work_id, node_id, prominence, added_at)
                 VALUES ('{id}','{TAG}',1,'2026-01-01T00:00:00Z')"
            ))
            .await;
        }
    }

    async fn seed(&self) {
        self.exec(&format!(
            "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version)
             VALUES ('{ACCOUNT}','active','tagpage@example.invalid','adult',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ))
        .await;
        self.exec(&format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at,
                                 updated_at, version)
             VALUES ('{PSEUD}','{ACCOUNT}','hollowgast','Hollow Gast','listed',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ))
        .await;
        self.exec(&format!(
            "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at)
             VALUES ('{TAG}','tag','slow burn','slow burn','2026-01-01T00:00:00Z')"
        ))
        .await;
        self.work(PUBLIC_A, "'The Long Way Round'", "public", true).await;
        self.work(PUBLIC_B, "'A Second Tagged Work'", "public", true).await;
        self.work(PRIVATE_C, "'A Private Tagged Work'", "private", true).await;
        self.work("01957000-0000-4000-8000-0000000001d1", "'Untagged'", "public", false)
            .await;
    }
}

async fn get(app: axum::Router, uri: &str) -> (StatusCode, String) {
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
async fn a_tag_lists_its_reachable_works_without_a_script() {
    let harness = Harness::new("list").await;
    harness.seed().await;

    let (status, body) = get(harness.client(), "/tags/slow%20burn").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!body.contains("<script"), "no JavaScript required: {body}");
    assert!(body.contains("slow burn"), "the tag names itself: {body}");
    assert!(body.contains("The Long Way Round"), "{body}");
    assert!(body.contains("A Second Tagged Work"), "{body}");
    assert!(
        body.contains(&format!("/works/{PUBLIC_A}")),
        "each entry links the work page: {body}"
    );
    assert!(body.contains("2 works"), "the count is what it lists: {body}");
}

#[tokio::test]
async fn a_private_work_tagged_with_the_same_word_is_not_listed() {
    let harness = Harness::new("private").await;
    harness.seed().await;
    let (status, body) = get(harness.client(), "/tags/slow%20burn").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        !body.contains("A Private Tagged Work"),
        "the listing is what the reader may reach: {body}"
    );
    assert!(
        !body.contains("Untagged"),
        "a work without the tag is not in the tag's page: {body}"
    );
}

#[tokio::test]
async fn a_tag_with_nothing_behind_it_is_a_404_page() {
    let harness = Harness::new("empty").await;
    harness.seed().await;
    let (status, body) = get(harness.client(), "/tags/nothing-at-all").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(body.contains("Not found"), "{body}");
}

#[tokio::test]
async fn the_json_browse_api_still_answers_json() {
    let harness = Harness::new("api").await;
    harness.seed().await;
    let (status, body) = get(harness.client(), "/api/v1/tags/slow%20burn").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    serde_json::from_str::<serde_json::Value>(&body).expect("the API still answers JSON");
}
