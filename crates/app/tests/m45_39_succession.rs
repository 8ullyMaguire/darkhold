//! M45-39 — §11.11a succession over HTTP: the public document, the registry, and the 404s.
//!
//! What only the routes can get wrong:
//!
//! 1. **The document is readable with no session at all.** The reader who needs it is holding a dead
//!    link and may never have had an account here.
//! 2. **A plan with no export is refused** — the refusal §11.11a exists to make.
//! 3. **A non-operator gets 404, not 403**, for set, confirm and withdraw.
//! 4. **Resolution takes the longest matching prefix**, so a rescued work can override a host-wide
//!    pointer, and a path matches only at a segment boundary.
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
        "darkhold-m45-39-{tag}-{}-{:?}",
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

    async fn request(&mut self, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json");
        if !self.cookies.is_empty() {
            let cookie = self
                .cookies
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect::<Vec<String>>()
                .join("; ");
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

    async fn send(&mut self, method: &str, uri: &str, body: Value) -> (StatusCode, Value) {
        self.request(method, uri, Some(body)).await
    }

    async fn register(&mut self, handle: &str) -> String {
        let (status, body) = self
            .send(
                "POST",
                "/api/v1/auth/register",
                json!({
                    "email": format!("m45-39-{handle}@example.test"),
                    "password": PASSWORD,
                    "handle": handle,
                    "display_name": handle,
                    "age_band": "adult",
                }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "register {handle}: {body}");
        body["pseuds"][0]["handle"]
            .as_str()
            .unwrap_or(handle)
            .to_owned()
    }
}

struct Harness {
    _dir: PathBuf,
    tdb: TestDb,
    config: Config,
}

impl Harness {
    /// A harness where `operator` holds the operator role and `reader` does not.
    async fn new(tag: &str) -> Self {
        server::set_trust_proxy(false);
        let dir = scratch_dir(tag);
        let mut config = config_for(&dir);
        let tdb = TestDb::connect_with_dir(tag, &dir).await;
        let db = tdb.db().clone();

        let mut bootstrap = Client::new(server::build_router(AppState::new(config.clone(), db.clone())));
        bootstrap.register("operator").await;
        let account = account_of(&db, "operator").await;
        config.administration.operator_account_id = Some(account.into());

        let mut reader_client =
            Client::new(server::build_router(AppState::new(config.clone(), db.clone())));
        reader_client.register("reader").await;

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

    /// A signed-in client for the named handle.
    async fn signed_in(&self, handle: &str) -> Client {
        let mut client = self.client();
        let (status, body) = client
            .send(
                "POST",
                "/api/v1/auth/login",
                json!({
                    "email": format!("m45-39-{handle}@example.test"),
                    "password": PASSWORD,
                }),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "login {handle}: {body}");
        client
    }
}

async fn account_of(db: &darkhold_db::Database, handle: &str) -> uuid::Uuid {
    let sql = db.sql(
        "SELECT account_id FROM pseuds WHERE lower(handle) = lower(?)",
        "SELECT account_id::text AS account_id FROM pseuds WHERE lower(handle) = lower(?)",
    );
    let value: String = match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar(&sql)
            .bind(handle)
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("the account"),
        darkhold_db::Backend::Postgres => sqlx::query_scalar(&sql)
            .bind(handle)
            .fetch_one(db.postgres_pool().expect("pg"))
            .await
            .expect("the account"),
    };
    value.parse().expect("an account id is a uuid")
}

fn plan_body() -> Value {
    json!({
        "destination": "https://successor.example",
        "export_url": "https://successor.example/export",
        "contact": "mailto:ops@successor.example",
        "review_days": 60,
    })
}

#[tokio::test]
async fn the_document_is_public_and_answers_null_before_a_plan_exists() {
    let h = Harness::new("public").await;
    // No session, no cookie, no registration: this is the reader with a dead link.
    let mut stranger = h.client();

    let (status, body) = stranger.get("/api/v1/succession").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["plan"], Value::Null, "no plan yet: {body}");
    let not_built = body["not_built"].as_array().expect("not_built");
    assert!(
        not_built.iter().any(|item| item.as_str().unwrap_or_default().contains("mutual backup")),
        "the document says which halves are missing: {body}"
    );
}

#[tokio::test]
async fn an_operator_publishes_a_plan_and_a_stranger_can_read_it() {
    let h = Harness::new("publish").await;
    let mut operator = h.signed_in("operator").await;

    let (status, body) = operator
        .send("PUT", "/api/v1/succession", plan_body())
        .await;
    assert_eq!(status, StatusCode::OK, "set the plan: {body}");
    assert_eq!(body["plan"]["destination"], json!("https://successor.example"));
    assert_eq!(body["plan"]["review_days"], json!(60));
    assert_eq!(body["plan"]["stale"], json!(false), "just confirmed: {body}");
    assert!(
        body["plan"]["review_due"].as_str().is_some(),
        "the window has a due date: {body}"
    );

    let mut stranger = h.client();
    let (status, body) = stranger.get("/api/v1/succession").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["plan"]["export_url"],
        json!("https://successor.example/export"),
        "readable with no session at all: {body}"
    );
}

#[tokio::test]
async fn a_plan_without_an_export_is_refused() {
    let h = Harness::new("no-export").await;
    let mut operator = h.signed_in("operator").await;

    let mut body = plan_body();
    body["export_url"] = json!("");
    let (status, body) = operator.send("PUT", "/api/v1/succession", body).await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "an export is mandatory: {body}"
    );
    assert!(
        body["error"]["field_errors"]
            .get("export_url")
            .is_some(),
        "the error names the field: {body}"
    );
}

#[tokio::test]
async fn a_reader_who_is_not_the_operator_gets_404_rather_than_403() {
    let h = Harness::new("not-operator").await;
    let mut reader = h.signed_in("reader").await;

    for (method, uri, body) in [
        ("PUT", "/api/v1/succession", plan_body()),
        ("POST", "/api/v1/succession/confirm", json!({})),
        (
            "PUT",
            "/api/v1/succession/redirects",
            json!({ "host": "old.example", "to_url": "https://new.example" }),
        ),
    ] {
        let (status, response) = reader.send(method, uri, body).await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "{method} {uri} must not confirm it exists: {response}"
        );
    }

    let (status, _) = reader.request("DELETE", "/api/v1/succession", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn redirects_resolve_by_the_longest_matching_prefix() {
    let h = Harness::new("redirects").await;
    let mut operator = h.signed_in("operator").await;

    for (path, to) in [
        ("/", "https://archive.example/all"),
        ("/works", "https://archive.example/works"),
        ("/works/42", "https://archive.example/works/42"),
    ] {
        let (status, body) = operator
            .send(
                "PUT",
                "/api/v1/succession/redirects",
                json!({ "host": "OLD.example:443", "path": path, "to_url": to }),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "register {path}: {body}");
    }

    let mut stranger = h.client();
    // The specific rescue wins over the host-wide pointer, which wins over nothing.
    let (status, body) = stranger
        .get("/api/v1/succession/redirects?host=old.example&path=/works/42")
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["to"], json!("https://archive.example/works/42"));

    let (status, body) = stranger
        .get("/api/v1/succession/redirects?host=old.example&path=/works/7")
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["to"], json!("https://archive.example/works"));

    let (status, body) = stranger
        .get("/api/v1/succession/redirects?host=old.example&path=/unrelated")
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["to"], json!("https://archive.example/all"));

    // A prefix matches at a segment boundary only: `/works` must not claim `/workshop`.
    let (status, body) = stranger
        .get("/api/v1/succession/redirects?host=old.example&path=/workshop")
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the host-wide pointer still answers: {body}"
    );
    assert_eq!(body["to"], json!("https://archive.example/all"));

    // A host nobody registered is a 404, not an empty answer.
    let (status, _) = stranger
        .get("/api/v1/succession/redirects?host=unknown.example&path=/works")
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn confirming_nothing_is_a_404_and_withdrawing_empties_the_document() {
    let h = Harness::new("withdraw").await;
    let mut operator = h.signed_in("operator").await;

    let (status, body) = operator
        .send("POST", "/api/v1/succession/confirm", json!({}))
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a confirmation of nothing would publish a blank document: {body}"
    );

    let (status, _) = operator
        .send("PUT", "/api/v1/succession", plan_body())
        .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = operator
        .send("POST", "/api/v1/succession/confirm", json!({ "review_days": 3650 }))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["plan"]["review_days"],
        json!(365),
        "the window is clamped to the top of the range: {body}"
    );

    let (status, body) = operator.request("DELETE", "/api/v1/succession", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["plan"], Value::Null, "withdrawn: {body}");
}
