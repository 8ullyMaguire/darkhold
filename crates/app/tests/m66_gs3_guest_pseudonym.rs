//! M66 GS-3 — the guest identity is a pseudonym, and it owns what it writes.
//!
//! GS-1 created a guest *account*; GS-2 made registration upgrade it in place. Neither proved the thing GS-3 is actually
//! about, because GS-2's test wrote the bookmark with SQL — so "a guest's bookmark survives registration" was true of the
//! table and unproven of the product. Here the guest writes through the public door, then the identity is rebuilt from
//! nothing but the cookies, exactly as a browser would after a restart.
//!
//! Two properties, and one of them is a refusal:
//!
//! 1. **The session cookie is the whole identity.** A second client holding only the guest's cookies is the same reader —
//!    same account id, same pseud, and their bookmark is on their shelf. No account, no login, no server-side "browser"
//!    concept: drop the client and rebuild it from the `Set-Cookie` values.
//! 2. **A cookie never authorises a write** (the Concord rule this row inherits). The guest's session cookie alone cannot
//!    create a bookmark: the double-submit CSRF header must match. This matters most for guests, because a guest has no
//!    password to change if a third-party site makes their browser write — the cookie *is* the credential.
//!
//! And the seam: the row that GS-2 left open — that what the guest wrote through the door is what registration keeps.

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server::{self, set_trust_proxy};
use darkhold_app::state::AppState;
use darkhold_db::DatabaseConfig;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tower::ServiceExt;

/// A subject id that is a UUID, because PostgreSQL stores `subject_id` as one and SQLite as text.
const WORK: &str = "00000000-0000-4000-8000-00000000a001";
const PASSWORD: &str = "test-pass-1234";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-m66-gs3-{tag}-{}-{:?}",
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
    config.database = match std::env::var("DARKHOLD_TEST_PG_URL") {
        Ok(url) if !url.is_empty() => DatabaseConfig::new(url),
        _ => DatabaseConfig::new(format!(
            "sqlite://{}/darkhold.sqlite?mode=rwc",
            dir.display()
        )),
    };
    config
}

#[derive(Clone, Default)]
struct Jar(Vec<(String, String)>);

impl Jar {
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
            self.0.retain(|(k, _)| k != &name);
            if !value.is_empty() {
                self.0.push((name, value));
            }
        }
    }

    fn csrf(&self) -> Option<String> {
        self.0
            .iter()
            .find(|(k, _)| k == "darkhold_csrf")
            .map(|(_, v)| v.clone())
    }

    /// Leads with the cookies when the request is built: this is what "rebuild the browser from the jar" means.
    fn cookie_header(&self) -> String {
        self.0
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("; ")
    }
}

struct Browser {
    app: axum::Router,
    jar: Jar,
}

impl Browser {
    fn new(app: axum::Router) -> Self {
        Self {
            app,
            jar: Jar::default(),
        }
    }

    fn from_jar(app: axum::Router, jar: Jar) -> Self {
        Self { app, jar }
    }

    async fn send(
        &mut self,
        method: &str,
        uri: &str,
        body: Option<Value>,
        csrf: bool,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json");
        if !self.jar.0.is_empty() {
            builder = builder.header(header::COOKIE, self.jar.cookie_header());
        }
        if csrf {
            if let Some(token) = self.jar.csrf() {
                builder = builder.header("x-csrf-token", token);
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
        self.jar.capture(&response);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn post(&mut self, uri: &str, body: Value) -> (StatusCode, Value) {
        self.send("POST", uri, Some(body), true).await
    }

    async fn get(&mut self, uri: &str) -> (StatusCode, Value) {
        self.send("GET", uri, None, false).await
    }

    /// What a visitor's browser does: create the guest, then keep only the jar.
    async fn become_guest(&mut self, version: i64) -> Value {
        let (status, body) = self
            .post(
                "/api/v1/auth/guest",
                json!({ "consent": { "version": version } }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        body
    }

    async fn bookmark(&mut self, note: &str, csrf: bool) -> (StatusCode, Value) {
        self.send(
            "POST",
            "/api/v1/bookmarks",
            Some(json!({ "subject_type": "work", "subject_id": WORK, "note": note })),
            csrf,
        )
        .await
    }
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

    fn app(&self) -> axum::Router {
        server::build_router(AppState::new(self.config.clone(), self.tdb.db().clone()))
    }

    fn browser(&self) -> Browser {
        Browser::new(self.app())
    }
}

#[tokio::test]
async fn the_cookie_is_the_guest_and_it_owns_what_it_wrote() {
    let harness = Harness::new("cookie").await;
    let version = harness.config.consent.version;
    let mut visitor = harness.browser();
    let guest = visitor.become_guest(version).await;
    let account_id = guest["account"]["id"].as_str().expect("id").to_owned();
    assert_eq!(guest["account"]["is_guest"], json!(true), "{guest}");

    let (status, created) = visitor.bookmark("written as nobody", true).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");

    // A second browser holding nothing but the cookies: same reader, same shelf.
    let mut returning = Browser::from_jar(harness.app(), visitor.jar.clone());
    let (status, me) = returning.get("/api/v1/auth/me").await;
    assert_eq!(status, StatusCode::OK, "{me}");
    assert_eq!(
        me["account"]["id"],
        json!(account_id),
        "the cookie is the identity: {me}"
    );
    assert_eq!(me["account"]["is_guest"], json!(true), "{me}");

    let (status, shelf) = returning.get("/api/v1/bookmarks").await;
    assert_eq!(status, StatusCode::OK, "{shelf}");
    assert!(
        shelf["items"]
            .as_array()
            .expect("items")
            .iter()
            .any(|item| item["note"].as_str() == Some("written as nobody")),
        "the guest's bookmark is on the guest's shelf in the other browser: {shelf}"
    );
}

#[tokio::test]
async fn a_cookie_alone_cannot_write() {
    let harness = Harness::new("csrf").await;
    let version = harness.config.consent.version;
    let mut visitor = harness.browser();
    visitor.become_guest(version).await;

    // The session cookie, no CSRF header: exactly what a third-party page can cause a browser to send.
    let (status, refused) = visitor.bookmark("cross-site", false).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a cookie must never authorise a write, and a guest has no password to rotate: {refused}"
    );

    // And nothing was written, which is the part that matters if the guard is ever moved after the handler.
    let (_, shelf) = visitor.get("/api/v1/bookmarks").await;
    assert_eq!(
        shelf["items"].as_array().expect("items").len(),
        0,
        "a refused write leaves no row: {shelf}"
    );

    // With the token the same request succeeds, so the refusal above is the token and not a blanket denial of guests.
    let (status, created) = visitor.bookmark("same-site", true).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
}

#[tokio::test]
async fn what_the_guest_wrote_is_what_registration_keeps() {
    let harness = Harness::new("keep").await;
    let version = harness.config.consent.version;
    let mut visitor = harness.browser();
    let guest = visitor.become_guest(version).await;
    let account_id = guest["account"]["id"].as_str().expect("id").to_owned();

    let (status, _) = visitor.bookmark("read after signing up", true).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, registered) = visitor
        .post(
            "/api/v1/auth/register",
            json!({
                "email": "gs3@example.test",
                "password": PASSWORD,
                "handle": "Gs3",
                "display_name": "GS-3",
                "age_band": "adult",
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{registered}");
    assert_eq!(
        registered["account"]["id"],
        json!(account_id),
        "{registered}"
    );

    // Not "the account still owns a row": the reader logs in tomorrow and finds it.
    let mut tomorrow = harness.browser();
    let (status, session) = tomorrow
        .post(
            "/api/v1/auth/login",
            json!({ "email": "gs3@example.test", "password": PASSWORD }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let (status, shelf) = tomorrow.get("/api/v1/bookmarks").await;
    assert_eq!(status, StatusCode::OK, "{shelf}");
    assert!(
        shelf["items"]
            .as_array()
            .expect("items")
            .iter()
            .any(|item| item["note"].as_str() == Some("read after signing up")),
        "the bookmark the guest wrote through the door is there after registering: {shelf}"
    );
}
