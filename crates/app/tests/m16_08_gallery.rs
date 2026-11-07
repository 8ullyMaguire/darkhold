//! M16-08 — the gallery end to end: install, uninstall, update, rate, fork licence.
//!
//! The domain rules have their own unit tests. This file proves the store and the doors hold them: an install that is idempotent because
//! the reader clicked twice, a count that is a census rather than a total, an update that respects its policy and refuses to widen a grant,
//! a rating that needs an install, and a fork that cannot be relabelled out of a copyleft licence.
//!
//! Packages are inserted directly as `approved` with a valid manifest document. Going through the review workflow is M16-07's subject and
//! would make every test here a test of that too — the gallery does not care how a package was approved, only that it is.

use std::path::PathBuf;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use serde_json::{json, Value};
use test_support::TestDb;
use tower::ServiceExt;

const PASSWORD: &str = "test-pass-1234";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-m1608-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
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
        let mut config = Config::development_defaults();
        config.storage.root = dir.clone();
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

    /// Insert a published package with a real manifest, so `install` has something to read.
    async fn publish(
        &self,
        id: &str,
        version: &str,
        license: &str,
        permissions: &[&str],
    ) -> String {
        let document = json!({
            "id": id,
            "version": 1,
            "category": "integration",
            "entrypoint": "main.wasm",
            "required_host_api_version": 1,
            "permissions": permissions,
            "resource_limits": { "memory_mib": 32, "fuel": 1_000_000, "host_calls_per_run": 64 },
            "supported_surfaces": ["library"],
            "license": license,
            "network_allowlist": [],
            "pricing": "free",
        })
        .to_string();
        sqlx::query(
            "INSERT INTO extension_manifests (id, version, document, submitted_by, state, created_at)
             VALUES (?, ?, ?, 'host', 'approved', ?)",
        )
        .bind(id)
        .bind(version)
        .bind(&document)
        // Ordered timestamps make "newest published version" deterministic in a test that publishes two releases.
        .bind(format!("2026-11-0{}T00:00:00Z", if version == "1.0.0" { 1 } else { 2 }))
        .execute(self.tdb.db().sqlite_pool().expect("sqlite"))
        .await
        .expect("insert manifest");
        document
    }
}

struct Client {
    app: axum::Router,
    cookies: Vec<(String, String)>,
    pseud: String,
}

impl Client {
    fn new(app: axum::Router) -> Self {
        Self {
            app,
            cookies: Vec::new(),
            pseud: String::new(),
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

    async fn send(&mut self, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut builder = Request::builder()
            .method(Method::from_bytes(method.as_bytes()).expect("method"))
            .uri(uri);
        if body.is_some() {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
        }
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
        let body = body.map_or_else(Body::empty, |value| Body::from(value.to_string()));
        let response = self
            .app
            .clone()
            .oneshot(builder.body(body).expect("request"))
            .await
            .expect("response");
        let status = response.status();
        self.capture(&response);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// Register and remember the pseud the session acts as.
    async fn register(&mut self, handle: &str) -> String {
        let (status, body) = self
            .send(
                "POST",
                "/api/v1/auth/register",
                Some(json!({
                    "email": format!("m16-08-{handle}@example.test"),
                    "password": PASSWORD,
                    "handle": handle,
                    "display_name": handle,
                    "age_band": "adult",
                })),
            )
            .await;
        assert!(
            status == StatusCode::CREATED || status == StatusCode::OK,
            "register {handle}: {body}"
        );
        let (status, me) = self.send("GET", "/api/v1/auth/me", None).await;
        assert_eq!(status, StatusCode::OK, "{me}");
        self.pseud = me["pseuds"][0]["id"].as_str().expect("pseud id").to_owned();
        me["account"]["id"]
            .as_str()
            .or_else(|| me["id"].as_str())
            .expect("account id")
            .to_owned()
    }
}

fn message(body: &Value) -> String {
    body["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

#[tokio::test]
async fn installing_twice_is_one_install_and_the_count_is_a_census() {
    let h = Harness::new("idempotent").await;
    h.publish("shelf-sorter", "1.0.0", "MIT", &["work.read"])
        .await;

    let mut reader = h.client();
    reader.register("reader-one").await;

    let (status, body) = reader
        .send(
            "POST",
            "/api/v1/extensions/shelf-sorter/install",
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["installed"], true, "{body}");
    assert_eq!(body["version"], "1.0.0", "{body}");
    assert_eq!(body["install_count"], 1, "{body}");
    assert_eq!(body["update_policy"], "pinned", "{body}");

    // The retry a client makes when it did not read the first response.
    let (status, body) = reader
        .send(
            "POST",
            "/api/v1/extensions/shelf-sorter/install",
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["installed"], false, "{body}");
    assert_eq!(body["already_installed"], true, "{body}");
    assert_eq!(
        body["install_count"], 1,
        "reinstalling is not a second install: {body}"
    );

    let (status, body) = reader
        .send("DELETE", "/api/v1/extensions/shelf-sorter/install", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["uninstalled"], true, "{body}");
    assert_eq!(body["install_count"], 0, "{body}");

    // And back again: the count follows reality, which is why it is computed rather than accumulated.
    let (status, body) = reader
        .send(
            "POST",
            "/api/v1/extensions/shelf-sorter/install",
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["install_count"], 1, "{body}");
}

#[tokio::test]
async fn two_scopes_are_two_installs() {
    let h = Harness::new("scopes").await;
    h.publish("shelf-sorter", "1.0.0", "MIT", &[]).await;

    let mut first = h.client();
    first.register("reader-a").await;
    let mut second = h.client();
    second.register("reader-b").await;

    first
        .send(
            "POST",
            "/api/v1/extensions/shelf-sorter/install",
            Some(json!({})),
        )
        .await;
    let (status, body) = second
        .send(
            "POST",
            "/api/v1/extensions/shelf-sorter/install",
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["install_count"], 2, "{body}");

    // The caller's own list names only its own installation.
    let (status, body) = first.send("GET", "/api/v1/me/installations", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["installations"].as_array().map(Vec::len),
        Some(1),
        "{body}"
    );
    assert_eq!(
        body["installations"][0]["manifest_id"], "shelf-sorter",
        "{body}"
    );
}

#[tokio::test]
async fn an_update_follows_the_policy_and_never_widens_a_grant_silently() {
    let h = Harness::new("update").await;
    h.publish("shelf-sorter", "1.0.0", "MIT", &["work.read"])
        .await;

    let mut reader = h.client();
    reader.register("updater").await;
    reader
        .send(
            "POST",
            "/api/v1/extensions/shelf-sorter/install",
            Some(json!({})),
        )
        .await;

    // A new release that asks for nothing new.
    h.publish("shelf-sorter", "1.1.0", "MIT", &["work.read"])
        .await;
    let (status, body) = reader
        .send(
            "POST",
            "/api/v1/extensions/shelf-sorter/update",
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["updated"], false,
        "a pinned install does not follow a release: {body}"
    );
    assert_eq!(body["version"], "1.0.0", "{body}");

    let (status, body) = reader
        .send(
            "POST",
            "/api/v1/extensions/shelf-sorter/update",
            Some(json!({ "update_policy": "minor" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["updated"], true, "{body}");
    assert_eq!(body["from"], "1.0.0", "{body}");
    assert_eq!(body["version"], "1.1.0", "{body}");

    // A release that asks for more: the update is refused and the refusal names what is new.
    h.publish("shelf-sorter", "1.2.0", "MIT", &["work.read", "network"])
        .await;
    let (status, body) = reader
        .send(
            "POST",
            "/api/v1/extensions/shelf-sorter/update",
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        message(&body).contains("network"),
        "the prompt names what is new: {}",
        message(&body)
    );
    assert!(
        !message(&body).contains("work.read"),
        "it names only what is new, not what was already granted: {}",
        message(&body)
    );
}

#[tokio::test]
async fn a_rating_needs_an_install_and_one_scope_is_one_rating() {
    let h = Harness::new("rating").await;
    h.publish("shelf-sorter", "1.0.0", "MIT", &[]).await;

    let mut reader = h.client();
    reader.register("rater").await;

    // Before installing there is nothing to have an opinion about.
    let (status, body) = reader
        .send(
            "PUT",
            "/api/v1/extensions/shelf-sorter/rating",
            Some(json!({ "stars": 5 })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        message(&body).contains("only a scope that has installed"),
        "{}",
        message(&body)
    );

    reader
        .send(
            "POST",
            "/api/v1/extensions/shelf-sorter/install",
            Some(json!({})),
        )
        .await;

    let (status, body) = reader
        .send(
            "PUT",
            "/api/v1/extensions/shelf-sorter/rating",
            Some(json!({ "stars": 5, "review": "Finds the good stuff." })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["score"], "5.0", "{body}");
    assert_eq!(body["count"], 1, "{body}");

    // Re-rating replaces the opinion rather than adding a second one.
    let (status, body) = reader
        .send(
            "PUT",
            "/api/v1/extensions/shelf-sorter/rating",
            Some(json!({ "stars": 3, "review": "Finds the good stuff, slowly." })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["score"], "3.0", "{body}");
    assert_eq!(body["count"], 1, "{body}");

    // Zero is not an abstention.
    let (status, body) = reader
        .send(
            "PUT",
            "/api/v1/extensions/shelf-sorter/rating",
            Some(json!({ "stars": 0 })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    let (status, body) = reader
        .send("GET", "/api/v1/extensions/shelf-sorter/ratings", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["count"], 1, "{body}");
    assert_eq!(body["install_count"], 1, "{body}");
    let reviews = body["reviews"].as_array().expect("reviews");
    assert_eq!(reviews.len(), 1, "{body}");
    assert_eq!(reviews[0]["stars"], 3, "{body}");
    assert_eq!(
        reviews[0]["version_rated"], "1.0.0",
        "a review says which version it is about: {body}"
    );
}

#[tokio::test]
async fn a_fork_cannot_be_relabelled_out_of_a_copyleft_licence() {
    let h = Harness::new("licence").await;
    h.publish("share-alike", "1.0.0", "CC-BY-SA-4.0", &[]).await;

    let mut author = h.client();
    author.register("forker").await;

    let (status, body) = author
        .send(
            "POST",
            "/api/v1/extensions/share-alike/fork",
            Some(json!({ "id": "share-alike-mine", "version": "1.0.0", "license": "MIT" })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(message(&body).contains("copyleft"), "{}", message(&body));

    // Keeping the licence is allowed, and the fork records where it came from.
    let (status, body) = author
        .send(
            "POST",
            "/api/v1/extensions/share-alike/fork",
            Some(
                json!({ "id": "share-alike-mine", "version": "1.0.0", "license": "CC-BY-SA-4.0" }),
            ),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["derived_from"], "share-alike", "{body}");
    assert!(
        body["attribution"]
            .as_str()
            .is_some_and(|text| text.contains("share-alike") && text.contains("CC-BY-SA-4.0")),
        "a fork says what it is based on and under which licence: {body}"
    );

    // And a permissive source may be forked under anything that names it.
    h.publish("mitten", "1.0.0", "MIT", &[]).await;
    let (status, body) = author
        .send(
            "POST",
            "/api/v1/extensions/mitten/fork",
            Some(json!({ "id": "mitten-gpl", "version": "1.0.0", "license": "GPL-3.0-only" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let document = darkhold_db::marketplace::manifest_document(h.tdb.db(), "mitten-gpl", "1.0.0")
        .await
        .expect("read")
        .expect("document");
    assert!(
        document.contains("GPL-3.0-only"),
        "the relabelled licence is what the fork's manifest says: {document}"
    );
}
