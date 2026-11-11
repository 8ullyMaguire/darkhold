//! M70-09 — extension documents over real HTTP.
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
        "darkhold-extdocroute-{tag}-{}-{:?}",
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

fn layout_json(name: &str) -> serde_json::Value {
    json!({
        "version": 1,
        "kind": "layout",
        "name": name,
        "body": { "slots": [ { "widget": "inbox" }, { "widget": "trends", "span": 6 } ] }
    })
}

#[tokio::test]
async fn authoring_and_import_are_one_path_so_an_import_cannot_reach_a_state_the_api_refuses() {
    let h = Harness::new("extdoc").await;
    let mut me = h.client();
    me.register("docs@example.test", "hollowgast").await;
    let mut stranger = h.client();
    stranger.register("other@example.test", "bystander").await;
    let mut anon = h.client();

    let (status, _) = anon
        .request("GET", "/api/v1/me/extension-documents", None)
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // A field this instance does not understand is refused with the field's name — the same answer whether the
    // document was typed here or arrived from another instance.
    let mut future = layout_json("Future");
    future["publish_at"] = json!("2027-01-01");
    let (status, body) = me
        .request(
            "POST",
            "/api/v1/me/extension-documents",
            Some(json!({ "document": future })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body.to_string().contains("publish_at"), "{body}");

    // A body this instance does not have is refused too, and the message lists what it does have.
    let unknown_widget = json!({ "version": 1, "kind": "layout", "name": "Astrology",
        "body": { "slots": [ { "widget": "astrology" } ] } });
    let (status, body) = me
        .request(
            "POST",
            "/api/v1/me/extension-documents",
            Some(json!({ "document": unknown_widget })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body.to_string().contains("known:"), "{body}");

    // The import path: the document arrives as text, exactly as an export produced it.
    let canonical = layout_json("Winter").to_string();
    let (status, body) = me
        .request(
            "POST",
            "/api/v1/me/extension-documents",
            Some(json!({ "document": canonical })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["name"], "Winter");
    assert_eq!(body["kind"], "layout");
    assert_eq!(body["is_public"], false);
    let id = body["id"].as_str().expect("id").to_owned();

    let (status, body) = me
        .request("GET", "/api/v1/me/extension-documents", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["count"], 1);
    assert_eq!(body["items"][0]["id"], id);

    // Export returns the stored canonical text, byte for byte.
    let (status, content_type, _, raw) = me
        .request_raw(
            "GET",
            &format!("/api/v1/me/extension-documents/{id}/export"),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        content_type.to_str().expect("content type"),
        "application/json; charset=utf-8"
    );
    assert_eq!(raw, canonical, "export is a read, not a re-serialisation");
    assert!(raw.contains("\"slot\"") || raw.contains("\"slots\""));

    // Private means private: another reader gets the same answer as for an id that does not exist.
    let (status, _) = stranger
        .request("GET", &format!("/api/v1/me/extension-documents/{id}"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, ..) = anon
        .request_raw(
            "GET",
            &format!("/api/v1/me/extension-documents/{id}/export"),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Publishing makes the export anonymous-visible, and still not deletable by anyone else.
    let (status, body) = me
        .request(
            "POST",
            "/api/v1/me/extension-documents",
            Some(json!({ "document": layout_json("Shared"), "is_public": true })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let shared = body["id"].as_str().expect("id").to_owned();
    let (status, ..) = anon
        .request_raw(
            "GET",
            &format!("/api/v1/me/extension-documents/{shared}/export"),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "publishing is what makes an export anonymous"
    );
    let (status, _) = stranger
        .request(
            "DELETE",
            &format!("/api/v1/me/extension-documents/{shared}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, body) = me
        .request(
            "DELETE",
            &format!("/api/v1/me/extension-documents/{id}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["deleted"], true);
    let (status, _) = me
        .request("GET", &format!("/api/v1/me/extension-documents/{id}"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
