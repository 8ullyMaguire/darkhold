//! M47-13 §9.7.9 — reading goals over HTTP.
//!
//! The store's properties are proved in `crates/db/tests/reading_goal.rs`. This file proves what only
//! the route can get wrong:
//!
//! 1. **No goal is `goal: null`, not `404`** — most readers have not set one, and a 404 would make
//!    every dashboard render an error before the reader did anything wrong.
//! 2. **The answer carries no verdict** — no `failed`, `missed` or `streak` key, asserted on the
//!    serialised JSON rather than on a struct.
//! 3. **An unknown period or metric is `422` naming the accepted values.**
//! 4. **Saving a chapter through the reader's own progress route moves the goal**, which is the one
//!    assertion that a `reading_unit_read` row is actually written by the product and not only by a
//!    fixture.
//!
//! Both engines: `TestDb` is SQLite by default and PostgreSQL under `DARKHOLD_TEST_PG_URL`.

use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::DatabaseConfig;
use serde_json::{json, Value};
use test_support::TestDb;
use tower::ServiceExt;

const PASSWORD: &str = "test-pass-1234";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-m47-13-{tag}-{}-{:?}",
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

    fn cookie_header(&self) -> Option<String> {
        if self.cookies.is_empty() {
            return None;
        }
        Some(
            self.cookies
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect::<Vec<String>>()
                .join("; "),
        )
    }

    fn capture(&mut self, response: &axum::response::Response) {
        for value in response.headers().get_all(header::SET_COOKIE) {
            let Ok(text) = value.to_str() else { continue };
            let Some((pair, _)) = text.split_once(';') else {
                continue;
            };
            let Some((name, value)) = pair.split_once('=') else {
                continue;
            };
            let name = name.trim().to_owned();
            let value = value.trim().to_owned();
            self.cookies.retain(|(k, _)| k != &name);
            if !value.is_empty() {
                self.cookies.push((name, value));
            }
        }
    }

    async fn request(
        &mut self,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(cookie) = self.cookie_header() {
            builder = builder.header(header::COOKIE, cookie);
        }
        if let Some(token) = self
            .cookies
            .iter()
            .find(|(k, _)| k == "darkhold_csrf")
            .map(|(_, v)| v.to_owned())
        {
            builder = builder.header("x-csrf-token", token);
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
        let status = response.status();
        self.capture(&response);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, json)
    }

    async fn get(&mut self, uri: &str) -> (StatusCode, Value) {
        self.request("GET", uri, None).await
    }

    async fn post(&mut self, uri: &str, body: Value) -> (StatusCode, Value) {
        self.request("POST", uri, Some(body)).await
    }

    async fn put(&mut self, uri: &str, body: Value) -> (StatusCode, Value) {
        self.request("PUT", uri, Some(body)).await
    }

    async fn delete(&mut self, uri: &str) -> (StatusCode, Value) {
        self.request("DELETE", uri, None).await
    }

    fn signed_out(&self) -> Client {
        Client {
            app: self.app.clone(),
            cookies: Vec::new(),
        }
    }

    async fn register(&mut self, handle: &str) {
        let (status, body) = self
            .post(
                "/api/v1/auth/register",
                json!({
                    "email": format!("m47-13-{handle}@example.test"),
                    "password": PASSWORD,
                    "handle": handle,
                    "display_name": handle,
                    "age_band": "adult",
                }),
            )
            .await;
        assert!(
            status == StatusCode::CREATED || status == StatusCode::OK,
            "register {handle}: {body}"
        );
    }
}

struct Harness {
    _dir: PathBuf,
    tdb: TestDb,
    config: Config,
}

impl Harness {
    async fn new(tag: &str) -> Self {
        server::set_trust_proxy(false);
        let dir = scratch_dir(tag);
        let tdb = TestDb::connect_with_dir(tag, &dir).await;
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
}

#[tokio::test]
async fn no_goal_is_null_rather_than_an_error() {
    let h = Harness::new("none").await;
    let mut reader = h.client();
    reader.register("reader").await;

    let (status, body) = reader.get("/api/v1/reading-goal").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["goal"],
        Value::Null,
        "opt-in means absent, not an error"
    );

    // And anonymously it is a 401, not a null: the goal is a pseud's.
    let mut anon = reader.signed_out();
    let (status, _) = anon.get("/api/v1/reading-goal").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn setting_a_goal_answers_its_progress_and_no_verdict() {
    let h = Harness::new("set").await;
    let mut reader = h.client();
    reader.register("reader").await;

    let (status, body) = reader
        .put(
            "/api/v1/reading-goal",
            json!({ "period": "daily", "metric": "chapters", "target": 3 }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["goal"]["period"], json!("daily"));
    assert_eq!(body["goal"]["metric"], json!("chapters"));
    assert_eq!(body["goal"]["target"], json!(3));
    assert_eq!(body["goal"]["scope"], json!("pseud"));
    assert_eq!(body["progress"]["current"], json!(0));
    assert_eq!(body["progress"]["remaining"], json!(3));
    assert_eq!(body["progress"]["met"], json!(false));

    // No word for failing anywhere in the payload: §9.7.1 forbids loss-framed language, and a key
    // that does not exist cannot grow a template that uses it.
    let text = body.to_string();
    for forbidden in ["failed", "missed", "streak", "behind", "lapsed"] {
        assert!(
            !text.contains(forbidden),
            "the payload must not carry {forbidden}: {text}"
        );
    }

    let (status, body) = reader.delete("/api/v1/reading-goal").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["removed"], json!(true));
    let (_, body) = reader.get("/api/v1/reading-goal").await;
    assert_eq!(body["goal"], Value::Null);
}

#[tokio::test]
async fn an_unknown_period_or_metric_names_what_is_accepted() {
    let h = Harness::new("invalid").await;
    let mut reader = h.client();
    reader.register("reader").await;

    let (status, body) = reader
        .put(
            "/api/v1/reading-goal",
            json!({ "period": "monthly", "metric": "chapters", "target": 3 }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let message = body["error"]["field_errors"]["period"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(
        message.contains("daily") && message.contains("weekly"),
        "the refusal names the periods: {body}"
    );

    let (status, body) = reader
        .put(
            "/api/v1/reading-goal",
            json!({ "period": "daily", "metric": "pages", "target": 3 }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let message = body["error"]["field_errors"]["metric"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(
        message.contains("chapters") && message.contains("words"),
        "the refusal names the metrics: {body}"
    );

    let (status, body) = reader
        .put(
            "/api/v1/reading-goal",
            json!({ "period": "daily", "metric": "chapters", "target": 0 }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["error"]["field_errors"]["target"].is_string());
}

#[tokio::test]
async fn reading_a_chapter_through_the_reader_route_moves_the_goal() {
    let h = Harness::new("moves").await;
    let mut author = h.client();
    author.register("author").await;

    // A work with one chapter, so the progress route has something real to point at.
    let (status, work) = author
        .post("/api/v1/works", json!({ "title": "A Work" }))
        .await;
    assert!(status.is_success(), "create the work: {work}");
    let work_id = work["id"].as_str().expect("the work id").to_owned();
    let (status, chapter) = author
        .post(
            &format!("/api/v1/works/{work_id}/chapters"),
            json!({ "title": "Chapter One" }),
        )
        .await;
    assert!(status.is_success(), "create the chapter: {chapter}");
    let unit_id = chapter["id"].as_str().expect("the chapter id").to_owned();

    let mut reader = h.client();
    reader.register("reader").await;
    let (status, body) = reader
        .put(
            "/api/v1/reading-goal",
            json!({ "period": "daily", "metric": "chapters", "target": 2 }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = reader
        .put(
            "/api/v1/reading/progress",
            json!({
                "subject_type": "work",
                "subject_id": work_id,
                "unit_id": unit_id,
                "position_permille": 1000,
            }),
        )
        .await;
    assert!(status.is_success(), "save progress: {body}");

    let (_, body) = reader.get("/api/v1/reading-goal").await;
    assert_eq!(
        body["progress"]["current"],
        json!(1),
        "the reader's own progress route is what records the chapter: {body}"
    );
    assert_eq!(body["progress"]["remaining"], json!(1));
}
