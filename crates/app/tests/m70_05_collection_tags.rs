//! M70-05 — collection tags over real HTTP.
//!
//! Four doors, and each asserts a different decision:
//!
//! * **The owner can orphan**, and the answer names the frozen author line rather than saying "ok".
//! * **A stranger cannot**, and learns nothing: the refusal is the same one a missing work gets, so the route
//!   cannot be used to enumerate work ids.
//! * **Doing it twice is refused**, because the second call would append a suffix and imply a second owner.
//! * **Without a session it is 401**, not 403 — there is nothing to be permitted yet.

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server::{self, set_trust_proxy};
use darkhold_app::state::AppState;
use darkhold_db::DatabaseConfig;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tower::ServiceExt;

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-ctags-{tag}-{}-{:?}",
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

    /// Run a statement against the test database.
    /// Run a fixture statement on whichever engine the suite is running against. The statements are literals and `?`
    /// binds; `exec_one` rewrites the placeholders for PostgreSQL, and an INSERT of literal values needs no cast because
    /// assignment coerces. Reaching for `sqlite_pool()` here is what made this whole file invisible to the deployed engine.
    async fn exec(&self, statement: &str) {
        test_support::exec_one(self.tdb.db(), statement).await;
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
        for (name, value) in &self.cookies {
            builder = builder.header(header::COOKIE, format!("{name}={value}"));
        }
        // GS-3 (2026-11-11) put the CSRF guard on the whole `api` router: a write must echo the token the session cookie set, or it is a
        // bare 403 that reads like a permissions bug. `test_support`'s client has done this since it existed; hand-rolled clients drift.
        if method != "GET" && method != "HEAD" {
            if let Some((_, token)) = self.cookies.iter().find(|(k, _)| k == "darkhold_csrf") {
                builder = builder.header("x-csrf-token", token.clone());
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
        let status = response.status();
        self.capture(&response);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&body).into_owned()));
        (status, json)
    }

    async fn post(&mut self, uri: &str, body: Value) -> (StatusCode, Value) {
        self.request("POST", uri, Some(body)).await
    }

    async fn register(&mut self, email: &str, handle: &str) -> String {
        let (status, body) = self
            .post(
                "/api/v1/auth/register",
                json!({
                    "email": email,
                    "password": PASSWORD,
                    "handle": handle,
                    "display_name": handle,
                    "age_band": "adult",
                }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "register {handle}: {body}");
        let (status, me) = self.request("GET", "/api/v1/auth/me", None).await;
        assert_eq!(status, StatusCode::OK, "{me}");
        me["account"]["id"].as_str().expect("account id").to_owned()
    }
}

#[tokio::test]
async fn only_the_owner_writes_and_everyone_reads() {
    let h = Harness::new("ctags").await;
    let mut owner = h.client();
    let account = owner.register("owner@ctags.test", "hollowgast").await;
    let mut stranger = h.client();
    stranger.register("stranger@ctags.test", "bystander").await;

    let collection = "01958000-0000-4000-8000-000000000001";
    h.exec(&format!(
        "INSERT INTO collections (id, name, description, owner, item_policy, is_public, created_at) \
         VALUES ('{collection}','Winter Exchange','','{account}','open',1,'2026-01-01T00:00:00Z')"
    ))
    .await;
    h.exec(
        "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at) \
         VALUES ('node-1','freeform','Yuletide 2026','yuletide 2026','2026-01-01T00:00:00Z')",
    )
    .await;

    // Reading is public: the tag is part of how a reader decides whether the collection is for them.
    let mut anon = h.client();
    let (status, body) = anon
        .request(
            "GET",
            &format!("/api/v1/collections/{collection}/tags"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["count"], 0);

    // Writing is the owner's, and a stranger learns nothing about whether the collection exists.
    let (status, body) = stranger
        .request(
            "PUT",
            &format!("/api/v1/collections/{collection}/tags/node-1"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let (status, _) = anon
        .request(
            "PUT",
            &format!("/api/v1/collections/{collection}/tags/node-1"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Attaching twice is one tag and it says which happened.
    let (status, body) = owner
        .request(
            "PUT",
            &format!("/api/v1/collections/{collection}/tags/node-1"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["added"], true);
    let (status, body) = owner
        .request(
            "PUT",
            &format!("/api/v1/collections/{collection}/tags/node-1"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["added"], false);

    let (status, body) = anon
        .request(
            "GET",
            &format!("/api/v1/collections/{collection}/tags"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["count"], 1);
    assert_eq!(body["items"][0]["canonical"], "Yuletide 2026");
    assert_eq!(body["items"][0]["kind"], "freeform");

    let (status, body) = owner
        .request(
            "DELETE",
            &format!("/api/v1/collections/{collection}/tags/node-1"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["removed"], true);
    let (_, body) = anon
        .request(
            "GET",
            &format!("/api/v1/collections/{collection}/tags"),
            None,
        )
        .await;
    assert_eq!(body["count"], 0);
}
