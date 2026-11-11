//! M70-06 — a reader's stylesheet over real HTTP.
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
        "darkhold-skinroute-{tag}-{}-{:?}",
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

    /// A GET whose response is not JSON: content type, cache policy and the exact bytes.
    async fn request_raw(
        &mut self,
        method: &str,
        uri: &str,
    ) -> (StatusCode, header::HeaderValue, Option<String>, String) {
        let mut builder = Request::builder().method(method).uri(uri);
        for (name, value) in &self.cookies {
            builder = builder.header(header::COOKIE, format!("{name}={value}"));
        }
        let response = self
            .app
            .clone()
            .oneshot(builder.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        self.capture(&response);
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .cloned()
            .unwrap_or_else(|| header::HeaderValue::from_static(""));
        let cache = response
            .headers()
            .get(header::CACHE_CONTROL)
            .map(|value| value.to_str().unwrap_or_default().to_owned());
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            status,
            content_type,
            cache,
            String::from_utf8_lossy(&body).into_owned(),
        )
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
async fn a_skin_is_validated_before_it_is_stored_and_served_only_to_its_reader() {
    let h = Harness::new("skin").await;
    let mut me = h.client();
    me.register("skin@example.test", "hollowgast").await;
    let mut anon = h.client();

    // Nobody sees a stylesheet without a session, and there is no account in the path to try.
    let (status, _) = anon.request("GET", "/api/v1/me/skin.css", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = anon.request("GET", "/api/v1/me/skin", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, body) = me.request("GET", "/api/v1/me/skin", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["present"], false);

    // A refusal carries the validator's own sentence, not a generic validation error.
    let (status, body) = me
        .request(
            "PUT",
            "/api/v1/me/skin",
            Some(json!({ "css": "@import url('https://evil.invalid/x.css');" })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let sentence = body.to_string();
    assert!(sentence.contains("@import"), "{sentence}");
    let (status, body) = me
        .request(
            "PUT",
            "/api/v1/me/skin",
            Some(json!({ "css": "body { background: url(https://track.invalid/p.gif); }" })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body.to_string().contains("data:"),
        "the refusal should say what is allowed: {body}"
    );

    // Nothing was stored by the refused writes.
    let (_, body) = me.request("GET", "/api/v1/me/skin", None).await;
    assert_eq!(body["present"], false);

    let css = ":root { --ink: #101014; }\nbody { color: var(--ink); }";
    let (status, body) = me
        .request("PUT", "/api/v1/me/skin", Some(json!({ "css": css })))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["bytes"], css.len());

    let (status, content_type, cache, raw) = me.request_raw("GET", "/api/v1/me/skin.css").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        content_type.to_str().expect("content type"),
        "text/css; charset=utf-8"
    );
    assert_eq!(
        cache.as_deref(),
        Some("private, no-cache"),
        "an edited skin must not lag behind its editor"
    );
    assert_eq!(raw, css, "served byte-for-byte as written");

    let (_, body) = me.request("GET", "/api/v1/me/skin", None).await;
    assert_eq!(body["present"], true);
    assert_eq!(body["url"], "/api/v1/me/skin.css");

    let (status, body) = me.request("DELETE", "/api/v1/me/skin", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["removed"], true);
    let (status, ..) = me.request_raw("GET", "/api/v1/me/skin.css").await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "no skin is a 404, not an empty stylesheet"
    );
}
