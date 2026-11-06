//! ADR 0006 — the landing page is a document, not a shell.
//!
//! Three assertions, each of which the SPA landing page failed:
//!
//! 1. an anonymous request gets HTML that **contains no script element** — the reader's browser needs nothing to read it;
//! 2. the page's starting works are real links to the server-rendered reader (`/works/{id}/units/{unit}`), not routes
//!    into an application;
//! 3. an empty catalogue renders no "Start reading" heading at all (§8.11), rather than an empty section;
//! 4. a signed-in reader is redirected to `/discover`, as the SPA did, so the document does not stand between a member
//!    and their own surface.

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
        "darkhold-landing-{tag}-{}-{:?}",
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

const WORK: &str = "01957000-0000-4000-8000-0000000000d0";
const UNIT: &str = "01956000-0000-4000-8000-0000000000c1";
const REVISION: &str = "01956000-0000-4000-8000-0000000000c2";
const PSEUD: &str = "01955000-0000-4000-8000-0000000000e1";
const ACCOUNT: &str = "01955000-0000-4000-8000-0000000000a1";

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

    /// One published, public work with one readable unit — the minimum a landing page can offer.
    async fn seed(&self) {
        self.exec(&format!(
            "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version)
             VALUES ('{ACCOUNT}','active','landing@example.invalid','adult',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ))
        .await;
        self.exec(&format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at,
                                 updated_at, version)
             VALUES ('{PSEUD}','{ACCOUNT}','hollowgast','hollowgast','listed',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ))
        .await;
        self.exec(&format!(
            "INSERT INTO works (id, owner_pseud_id, title, summary, language, rating, visibility,
                                lifecycle, completion, published_at, created_at, updated_at, version)
             VALUES ('{WORK}','{PSEUD}','The Long Way Round','A blurb.','en','general','public',
                     'published','complete','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',
                     '2026-01-01T00:00:00Z',1)"
        ))
        .await;
        self.exec(&format!(
            "INSERT INTO publication_events (id, work_id, action, occurred_at, actor_pseud_id)
             VALUES ('01959000-0000-4000-8000-0000000000f1','{WORK}','publish',
                     '2026-01-01T00:00:00Z','{PSEUD}')"
        ))
        .await;
        self.exec(&format!(
            "INSERT INTO units (id, work_id, order_key, title, current_revision_id, created_at,
                                updated_at, version)
             VALUES ('{UNIT}','{WORK}',10,'One',NULL,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ))
        .await;
        self.exec(&format!(
            "INSERT INTO revisions (id, unit_id, revision_number, document_json, sanitized_html,
                                    plain_text, word_count, created_by_pseud_id, created_at)
             VALUES ('{REVISION}','{UNIT}',1,'{{}}','<p>One.</p>','One.',120,'{PSEUD}',
                     '2026-01-01T00:00:00Z')"
        ))
        .await;
        self.exec(&format!(
            "UPDATE units SET current_revision_id = '{REVISION}' WHERE id = '{UNIT}'"
        ))
        .await;
    }
}

const PASSWORD: &str = "test-pass-1234";

struct Client {
    app: axum::Router,
    cookies: Vec<(String, String)>,
}

impl Client {
    fn new(app: axum::Router) -> Self {
        Self {
            app,
            cookies: Vec::new(),
        }
    }

    fn capture(&mut self, response: &axum::response::Response) {
        for value in response.headers().get_all(header::SET_COOKIE) {
            let Ok(text) = value.to_str() else { continue };
            let Some((pair, _)) = text.split_once(';') else {
                continue;
            };
            if let Some((name, value)) = pair.split_once('=') {
                let name = name.trim().to_owned();
                let value = value.trim().to_owned();
                self.cookies.retain(|(k, _)| k != &name);
                if !value.is_empty() {
                    self.cookies.push((name, value));
                }
            }
        }
    }

    async fn raw(&mut self, method: &str, uri: &str, body: Option<Value>) -> axum::response::Response {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json");
        for (name, value) in &self.cookies {
            builder = builder.header(header::COOKIE, format!("{name}={value}"));
            if name == "darkhold_csrf" {
                builder = builder.header("x-csrf-token", value.clone());
            }
        }
        let body = match body {
            Some(v) => Body::from(v.to_string()),
            None => Body::empty(),
        };
        let response = self
            .app
            .clone()
            .oneshot(builder.body(body).unwrap())
            .await
            .unwrap();
        self.capture(&response);
        response
    }

    async fn get(&mut self, uri: &str) -> (StatusCode, String) {
        let response = self.raw("GET", uri, None).await;
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    async fn post(&mut self, uri: &str, body: Value) -> (StatusCode, String) {
        let response = self.raw("POST", uri, Some(body)).await;
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8_lossy(&body).into_owned())
    }
}

#[tokio::test]
async fn the_landing_page_is_a_document_with_no_script() {
    let harness = Harness::new("document").await;
    harness.seed().await;
    let mut client = harness.client();

    let (status, body) = client.get("/").await;
    assert_eq!(status, StatusCode::OK, "{body}");

    assert!(
        !body.contains("<script"),
        "the landing page must read without JavaScript: {body}"
    );
    assert!(
        body.contains(&harness.config.site.name),
        "the instance's name is the page: {body}"
    );
    assert!(
        body.contains(&harness.config.site.tagline),
        "the instance's tagline is public (spec 12.1): {body}"
    );
    assert!(
        body.contains("The Long Way Round"),
        "a published work is offered: {body}"
    );
    assert!(
        body.contains(&format!("/works/{WORK}/units/{UNIT}")),
        "the starter links the server-rendered reader, not an application route: {body}"
    );
    assert!(
        body.contains("120 words"),
        "the word count comes from the readable revision: {body}"
    );
}

#[tokio::test]
async fn an_empty_catalogue_renders_no_start_reading_heading() {
    let harness = Harness::new("empty").await;
    let mut client = harness.client();

    let (status, body) = client.get("/").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        !body.contains("Start reading"),
        "absent, not empty (spec 8.11): {body}"
    );
    assert!(
        body.contains(&harness.config.site.name),
        "the declaration still renders: {body}"
    );
}

#[tokio::test]
async fn a_signed_in_reader_is_sent_to_discovery() {
    let harness = Harness::new("signed-in").await;
    let mut client = harness.client();

    // Registering gives the client its session cookie.
    let (status, body) = client
        .post(
            "/api/v1/auth/register",
            serde_json::json!({
                "email": "landing-reader@example.invalid",
                "password": PASSWORD,
                "handle": "landingreader",
                "display_name": "landingreader",
                "age_band": "adult",
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "register: {body}");

    let response = client.raw("GET", "/", None).await;
    assert!(
        response.status().is_redirection(),
        "a signed-in reader does not need the landing page: {}",
        response.status()
    );
    assert_eq!(
        response
            .headers()
            .get(header::LOCATION)
            .and_then(|v| v.to_str().ok()),
        Some("/discover"),
    );
}
