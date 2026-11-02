//! M47-12 — §9.4 quick reactions over HTTP.
//!
//! The store's properties are proved in `crates/db/tests/quick_reactions.rs`. This file proves the
//! four things only the route can get wrong:
//!
//! 1. **The tally is public and the vocabulary ships with it**, so the bar renders the labels the
//!    server will actually accept instead of a frontend copy that can drift.
//! 2. **`mine` is per-reader** and empty for an anonymous caller — a public tally must not leak who
//!    reacted.
//! 3. **An unknown label is 422 with the vocabulary named**, because "unknown label" alone leaves
//!    the reader guessing six slugs.
//! 4. **The author's feedback preference closes the bar**: `GET` answers `enabled: false` rather
//!    than an error (the page stays readable), and a write is `403`.
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
const NOW: &str = "2026-11-02T00:00:00Z";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-m47-12-{tag}-{}-{:?}",
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

    async fn put(&mut self, uri: &str) -> (StatusCode, Value) {
        self.request("PUT", uri, None).await
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
                    "email": format!("m47-12-{handle}@example.test"),
                    "password": PASSWORD,
                    "handle": handle,
                    "display_name": handle,
                    "age_band": "adult",
                }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "register {handle}: {body}");
    }

    /// Create a work and return its id.
    async fn work(&mut self, title: &str) -> String {
        let (status, body) = self.post("/api/v1/works", json!({ "title": title })).await;
        assert_eq!(status, StatusCode::CREATED, "create the work: {body}");
        body["id"].as_str().expect("the work's id").to_owned()
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

    async fn exec(&self, query: &str) {
        let q = self.tdb.sql(query);
        match self.tdb.db().backend() {
            darkhold_db::Backend::Sqlite => {
                sqlx::query(&q)
                    .execute(self.tdb.db().sqlite_pool().expect("sqlite"))
                    .await
                    .expect("exec");
            }
            darkhold_db::Backend::Postgres => {
                sqlx::query(&q)
                    .execute(self.tdb.db().postgres_pool().expect("pg"))
                    .await
                    .expect("exec");
            }
        }
    }
}

#[tokio::test]
async fn the_bar_is_public_and_ships_its_vocabulary() {
    let h = Harness::new("public").await;
    let mut author = h.client();
    author.register("author").await;
    let work = author.work("A Work").await;

    let mut anon = author.signed_out();
    let (status, body) = anon
        .get(&format!("/api/v1/works/{work}/quick-reactions"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["enabled"], json!(true));
    assert!(body["items"].as_array().expect("items").is_empty());
    assert!(body["mine"].as_array().expect("mine").is_empty());
    let vocabulary = body["vocabulary"].as_array().expect("vocabulary");
    assert_eq!(vocabulary.len(), 6, "the six §9.4 labels: {body}");
    assert!(vocabulary
        .iter()
        .any(|entry| entry[0] == json!("made_me_cry")));
}

#[tokio::test]
async fn reacting_counts_once_and_shows_only_to_its_owner() {
    let h = Harness::new("count").await;
    let mut author = h.client();
    author.register("author").await;
    let work = author.work("A Work").await;
    let mut reader = h.client();
    reader.register("reader").await;

    let uri = format!("/api/v1/works/{work}/quick-reactions/made_me_cry");
    let (status, body) = reader.put(&uri).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // A double-tap is one reaction, not a 409 and not a second row.
    let (status, body) = reader.put(&uri).await;
    assert_eq!(status, StatusCode::OK, "a double-tap is idempotent: {body}");

    let (_, body) = reader
        .get(&format!("/api/v1/works/{work}/quick-reactions"))
        .await;
    assert_eq!(body["items"][0]["label"], json!("made_me_cry"));
    assert_eq!(body["items"][0]["count"], json!(1));
    assert_eq!(body["mine"], json!(["made_me_cry"]));

    // The tally is public; `mine` is not.
    let mut anon = reader.signed_out();
    let (_, body) = anon
        .get(&format!("/api/v1/works/{work}/quick-reactions"))
        .await;
    assert_eq!(body["items"][0]["count"], json!(1));
    assert!(body["mine"].as_array().expect("mine").is_empty());
}

#[tokio::test]
async fn an_unknown_label_is_refused_with_the_vocabulary_named() {
    let h = Harness::new("label").await;
    let mut author = h.client();
    author.register("author").await;
    let work = author.work("A Work").await;

    let (status, body) = author
        .put(&format!("/api/v1/works/{work}/quick-reactions/insightful"))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let errors = body["error"]["field_errors"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    let message = errors
        .get("label")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_owned();
    assert!(
        message.contains("made_me_cry") && message.contains("worldbuilding"),
        "the refusal names the vocabulary: {body}"
    );
}

#[tokio::test]
async fn withdrawing_reports_whether_there_was_anything_to_withdraw() {
    let h = Harness::new("withdraw").await;
    let mut author = h.client();
    author.register("author").await;
    let work = author.work("A Work").await;
    let uri = format!("/api/v1/works/{work}/quick-reactions/comforting");

    let (status, body) = author.put(&uri).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = author.delete(&uri).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["removed"], json!(true));

    let (status, body) = author.delete(&uri).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "withdrawing twice is not an error: {body}"
    );
    assert_eq!(body["removed"], json!(false));
}

#[tokio::test]
async fn the_authors_preference_closes_the_bar_without_hiding_the_page() {
    let h = Harness::new("gate").await;
    let mut author = h.client();
    author.register("author").await;
    let work = author.work("A Work").await;

    // The work-level override the author's settings surface writes.
    // `comments_enabled` is `INTEGER 0/1` on SQLite and `BOOLEAN` on PostgreSQL, which rejects an
    // integer literal for a boolean column. Write the literal the engine accepts.
    let off = match h.tdb.db().backend() {
        darkhold_db::Backend::Sqlite => "0",
        darkhold_db::Backend::Postgres => "false",
    };
    h.exec(&format!(
        "INSERT INTO work_feedback_preferences
         (work_id, accept_constructive, ambiguous_auto_deliver, comments_enabled, author_note, updated_at, version)
         VALUES ('{work}', NULL, NULL, {off}, NULL, '{NOW}', 1)"
    ))
    .await;

    let (status, body) = author
        .get(&format!("/api/v1/works/{work}/quick-reactions"))
        .await;
    assert_eq!(status, StatusCode::OK, "the read still answers: {body}");
    assert_eq!(body["enabled"], json!(false));
    assert!(body["items"].as_array().expect("items").is_empty());

    let (status, _) = author
        .put(&format!("/api/v1/works/{work}/quick-reactions/banter"))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
