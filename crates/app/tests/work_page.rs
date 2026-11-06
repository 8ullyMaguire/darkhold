//! ADR 0006 / M71-20 — the work page is a document.
//!
//! A work page is what an archive links to. Five assertions, each a rule rather than a layout choice:
//!
//! 1. a public work renders title, credits, summary, tags, contents and a *start reading* link into the reader, with
//!    **no script element**;
//! 2. an unknown id is a 404 page, not an empty shell;
//! 3. an unlisted work is readable by URL (§8.1: unlisted is not secret from a reader who has the link);
//! 4. a private work answers exactly as a work that does not exist (§8.9) — same status, same words;
//! 5. a restricted work answers the same way while `works.gated_metadata_public` is off, which is its default.

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server::{self, set_trust_proxy};
use darkhold_app::state::AppState;
use darkhold_db::DatabaseConfig;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tower::ServiceExt;

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-workpage-{tag}-{}-{:?}",
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

const PUBLIC_WORK: &str = "01957000-0000-4000-8000-0000000000d1";
const UNLISTED_WORK: &str = "01957000-0000-4000-8000-0000000000d2";
const PRIVATE_WORK: &str = "01957000-0000-4000-8000-0000000000d3";
const RESTRICTED_WORK: &str = "01957000-0000-4000-8000-0000000000d4";
const UNIT_ONE: &str = "01956000-0000-4000-8000-0000000000c1";
const UNIT_TWO: &str = "01956000-0000-4000-8000-0000000000c2";
const REVISION_ONE: &str = "01956000-0000-4000-8000-0000000000e1";
const REVISION_TWO: &str = "01956000-0000-4000-8000-0000000000e2";
const PSEUD: &str = "01955000-0000-4000-8000-0000000000e1";
const ACCOUNT: &str = "01955000-0000-4000-8000-0000000000a1";
const TAG_NODE: &str = "01954000-0000-4000-8000-0000000000f1";

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

    fn client(&self) -> Client {
        Client::new(server::build_router(AppState::new(
            self.config.clone(),
            self.tdb.db().clone(),
        )))
    }

    async fn exec(&self, statement: &str) {
        sqlx::query(statement)
            .execute(self.tdb.db().sqlite_pool().expect("sqlite"))
            .await
            .unwrap_or_else(|error| panic!("{statement}\n{error}"));
    }

    async fn work(&self, id: &str, title: &str, visibility: &str) {
        self.exec(&format!(
            "INSERT INTO works (id, owner_pseud_id, title, summary, language, rating, visibility,
                                lifecycle, completion, published_at, created_at, updated_at, version)
             VALUES ('{id}','{PSEUD}',{title},'A blurb about the thing.','en','general','{visibility}',
                     'published','in_progress','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',
                     '2026-01-01T00:00:00Z',1)"
        ))
        .await;
        self.exec(&format!(
            "INSERT INTO publication_events (id, work_id, action, occurred_at, actor_pseud_id)
             VALUES (lower(hex(randomblob(16))),'{id}','publish','2026-01-01T00:00:00Z','{PSEUD}')"
        ))
        .await;
        self.exec(&format!(
            "INSERT INTO work_contributors (work_id, pseud_id, role, public_attribution, created_at)
             VALUES ('{id}','{PSEUD}','owner',1,'2026-01-01T00:00:00Z')"
        ))
        .await;
    }

    async fn seed(&self) {
        self.exec(&format!(
            "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version)
             VALUES ('{ACCOUNT}','active','workpage@example.invalid','adult',
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

        self.work(PUBLIC_WORK, "'The Long Way Round'", "public").await;
        self.work(UNLISTED_WORK, "'An Unlisted Thing'", "unlisted").await;
        self.work(PRIVATE_WORK, "'A Private Thing'", "private").await;
        self.work(RESTRICTED_WORK, "'A Restricted Thing'", "restricted")
            .await;

        // Two readable units on the public work, so "contents" has something to list and the action has a first link.
        for (unit, revision, order, title, words) in [
            (UNIT_ONE, REVISION_ONE, 10, "One", 1200),
            (UNIT_TWO, REVISION_TWO, 20, "Two", 800),
        ] {
            self.exec(&format!(
                "INSERT INTO units (id, work_id, order_key, title, current_revision_id, created_at,
                                    updated_at, version)
                 VALUES ('{unit}','{PUBLIC_WORK}',{order},'{title}',NULL,
                         '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
            ))
            .await;
            self.exec(&format!(
                "INSERT INTO revisions (id, unit_id, revision_number, document_json, sanitized_html,
                                        plain_text, word_count, created_by_pseud_id, created_at)
                 VALUES ('{revision}','{unit}',1,'{{}}','<p>Text.</p>','Text.',{words},'{PSEUD}',
                         '2026-01-01T00:00:00Z')"
            ))
            .await;
            self.exec(&format!(
                "UPDATE units SET current_revision_id = '{revision}' WHERE id = '{unit}'"
            ))
            .await;
        }

        // One tag, through the taxonomy the work page reads: work_tags holds node ids, so the node comes first.
        self.exec(&format!(
            "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at)
             VALUES ('{TAG_NODE}','tag','slow burn','slow burn','2026-01-01T00:00:00Z')"
        ))
        .await;
        self.exec(&format!(
            "INSERT INTO work_tags (work_id, node_id, prominence, added_at)
             VALUES ('{PUBLIC_WORK}','{TAG_NODE}',1,'2026-01-01T00:00:00Z')"
        ))
        .await;
    }
}

struct Client {
    app: axum::Router,
}

impl Client {
    fn new(app: axum::Router) -> Self {
        Self { app }
    }

    async fn get(&mut self, uri: &str) -> (StatusCode, String) {
        let response = self
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(uri)
                    .header(header::ACCEPT, "text/html")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8_lossy(&body).into_owned())
    }
}

#[tokio::test]
async fn a_public_work_renders_as_a_readable_document() {
    let harness = Harness::new("public").await;
    harness.seed().await;
    let mut client = harness.client();

    let (status, body) = client.get(&format!("/works/{PUBLIC_WORK}")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        !body.contains("<script"),
        "the work page must read without JavaScript: {body}"
    );
    assert!(body.contains("The Long Way Round"), "{body}");
    assert!(
        body.contains("Hollow Gast") && body.contains("/people/hollowgast"),
        "the credited author is a link: {body}"
    );
    assert!(body.contains("A blurb about the thing."), "{body}");
    assert!(body.contains("general"), "rating is stated: {body}");
    assert!(body.contains("2000 words"), "length is the readable total: {body}");
    assert!(body.contains("slow burn"), "tags are listed: {body}");
    assert!(
        body.contains(&format!("/works/{PUBLIC_WORK}/units/{UNIT_ONE}")),
        "the primary action and the contents link the reader: {body}"
    );
    assert!(
        body.contains("Start reading"),
        "the primary action is present: {body}"
    );
    assert!(
        body.contains(&format!("/works/{PUBLIC_WORK}/units/{UNIT_TWO}")),
        "every readable unit is listed: {body}"
    );
    test_support::assert_no_empty_section(&body, "work page");
}

#[tokio::test]
async fn an_unknown_id_is_a_404_page() {
    let harness = Harness::new("unknown").await;
    let mut client = harness.client();
    let (status, body) = client
        .get("/works/01957000-0000-4000-8000-0000000000ff")
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(body.contains("Not found"), "{body}");
}

#[tokio::test]
async fn an_unlisted_work_is_readable_by_url() {
    let harness = Harness::new("unlisted").await;
    harness.seed().await;
    let mut client = harness.client();
    let (status, body) = client.get(&format!("/works/{UNLISTED_WORK}")).await;
    assert_eq!(status, StatusCode::OK, "unlisted is not hidden from a link: {body}");
    assert!(body.contains("An Unlisted Thing"), "{body}");
}

#[tokio::test]
async fn a_private_work_is_indistinguishable_from_a_missing_one() {
    let harness = Harness::new("private").await;
    harness.seed().await;
    let mut client = harness.client();

    let (missing_status, missing_body) = client
        .get("/works/01957000-0000-4000-8000-0000000000fe")
        .await;
    let (private_status, private_body) = client.get(&format!("/works/{PRIVATE_WORK}")).await;

    assert_eq!(private_status, StatusCode::NOT_FOUND, "{private_body}");
    assert_eq!(
        private_status, missing_status,
        "a private work must answer with the status of a work that does not exist"
    );
    // The only difference permitted between the two bodies is none: both are the same page, because a different page
    // for a real id is a disclosure.
    assert_eq!(
        private_body.replace(PRIVATE_WORK, "ID"),
        missing_body.replace("01957000-0000-4000-8000-0000000000fe", "ID"),
        "the bodies must be identical apart from the id"
    );
}

#[tokio::test]
async fn a_restricted_work_shows_its_record_and_withholds_its_text() {
    let harness = Harness::new("restricted").await;
    harness.seed().await;
    let mut client = harness.client();
    let (status, body) = client.get(&format!("/works/{RESTRICTED_WORK}")).await;

    // M70-04: restricted is the one tier whose *record* is public on purpose — a stranger finds the work and is told
    // why they cannot read it. `denial` encodes that as `SignInRequired` -> `MetadataOnly`, before
    // `works.gated_metadata_public` is consulted at all, so this arm must not be gated on a config switch.
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("A Restricted Thing"), "the record is public: {body}");
    assert!(
        body.contains("text is held"),
        "the page must say plainly why there is no chapter here: {body}"
    );
    assert!(
        !body.contains("/units/"),
        "and it must not link a chapter the reader cannot open: {body}"
    );
}

#[tokio::test]
async fn the_json_api_is_untouched_by_the_document_route() {
    let harness = Harness::new("api").await;
    harness.seed().await;
    let mut client = harness.client();
    let (status, body) = client.get(&format!("/api/v1/works/{PUBLIC_WORK}")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let parsed: Value = serde_json::from_str(&body).expect("the API still answers JSON");
    assert_eq!(parsed["title"], "The Long Way Round");
}
