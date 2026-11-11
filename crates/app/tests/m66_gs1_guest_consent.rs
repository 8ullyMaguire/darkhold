//! M66 GS-1 — a reader may read before registering, and the consent gate exists **before** the surface it gates.
//!
//! Five properties, and the first one is the one that makes the other four worth having:
//!
//! 1. **Silence is a refusal.** A guest request with no accepted version creates nothing — no account, no session — and says
//!    which field was missing. A gate that answered "welcome" and recorded nothing would be decoration.
//! 2. **The record is written with the guest, in the same act**, and reports the version it accepted.
//! 3. **A guest is an account**: `is_guest` on the account view, a pseud to own bookmarks, and a trust level (the ladder's
//!    guest rung) rather than a special case every later feature has to remember.
//! 4. **Asking twice keeps the same guest** — the route creates guests, it does not mint one per page load.
//! 5. **The switch has a reader**: `consent.required_for_guest = false` skips the interstitial because an operator said so,
//!    which is the only thing that may skip it.
//!
//! And one boundary: a **registered** reader cannot become a guest. Upgrading is one direction.

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
        "darkhold-m66-gs1-{tag}-{}-{:?}",
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
        _ => DatabaseConfig::new(format!("sqlite://{}/darkhold.sqlite?mode=rwc", dir.display())),
    };
    config
}

const PASSWORD: &str = "test-pass-1234";

struct Client {
    app: axum::Router,
    cookies: Vec<(String, String)>,
}

impl Client {
    fn new(app: axum::Router) -> Self {
        Self { app, cookies: Vec::new() }
    }

    fn capture(&mut self, response: &axum::response::Response) {
        for value in response.headers().get_all(header::SET_COOKIE) {
            let Ok(text) = value.to_str() else { continue };
            let Some((pair, _)) = text.split_once(';') else { continue };
            let Some((name, value)) = pair.split_once('=') else { continue };
            let name = name.trim().to_owned();
            let value = value.trim().to_owned();
            self.cookies.retain(|(k, _)| k != &name);
            if !value.is_empty() {
                self.cookies.push((name, value));
            }
        }
    }

    fn has_session(&self) -> bool {
        self.cookies.iter().any(|(k, _)| k == "darkhold_session")
    }

    async fn send(&mut self, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json");
        for (name, value) in &self.cookies {
            builder = builder.header(header::COOKIE, format!("{name}={value}"));
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
        let response = self.app.clone().oneshot(builder.body(body).unwrap()).await.unwrap();
        let status = response.status();
        self.capture(&response);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    async fn post(&mut self, uri: &str, body: Value) -> (StatusCode, Value) {
        self.send("POST", uri, Some(body)).await
    }

    async fn get(&mut self, uri: &str) -> (StatusCode, Value) {
        self.send("GET", uri, None).await
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
        body["account"]["id"].as_str().expect("account id").to_owned()
    }
}

struct Harness {
    _dir: PathBuf,
    tdb: test_support::TestDb,
    config: Config,
}

impl Harness {
    async fn new(tag: &str) -> Self {
        Self::with(tag, config_for(&scratch_dir(tag))).await
    }

    async fn with(tag: &str, config: Config) -> Self {
        set_trust_proxy(false);
        let dir = config.storage.root.clone();
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        let tdb = test_support::TestDb::connect_with_dir(tag, &dir).await;
        Self { _dir: dir, tdb, config }
    }

    fn client(&self) -> Client {
        Client::new(server::build_router(AppState::new(
            self.config.clone(),
            self.tdb.db().clone(),
        )))
    }

    fn db(&self) -> &darkhold_db::Database {
        self.tdb.db()
    }

    /// How many guest accounts exist. The refusal test turns on this number, not on a status code alone.
    async fn guests(&self) -> i64 {
        use sqlx::Row;
        match self.db().backend() {
            darkhold_db::Backend::Sqlite => sqlx::query("SELECT COUNT(*) AS n FROM accounts WHERE is_guest = 1")
                .fetch_one(self.db().sqlite_pool().expect("sqlite"))
                .await
                .expect("count guests")
                .get::<i64, _>("n"),
            darkhold_db::Backend::Postgres => {
                sqlx::query("SELECT COUNT(*) AS n FROM accounts WHERE is_guest = 1")
                    .fetch_one(self.db().postgres_pool().expect("postgres"))
                    .await
                    .expect("count guests")
                    .get::<i64, _>("n")
            }
        }
    }

    async fn consent_rows(&self) -> i64 {
        use sqlx::Row;
        match self.db().backend() {
            darkhold_db::Backend::Sqlite => sqlx::query("SELECT COUNT(*) AS n FROM consents")
                .fetch_one(self.db().sqlite_pool().expect("sqlite"))
                .await
                .expect("count consents")
                .get::<i64, _>("n"),
            darkhold_db::Backend::Postgres => sqlx::query("SELECT COUNT(*) AS n FROM consents")
                .fetch_one(self.db().postgres_pool().expect("postgres"))
                .await
                .expect("count consents")
                .get::<i64, _>("n"),
        }
    }
}

#[tokio::test]
async fn silence_is_not_consent_and_creates_nothing() {
    let harness = Harness::new("silence").await;
    let mut reader = harness.client();

    let (status, body) = reader.post("/api/v1/auth/guest", json!({})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{status} {body}");
    assert!(
        body["error"]["field_errors"].get("consent").is_some(),
        "the refusal names the field that was missing: {body}"
    );
    assert!(
        body["error"]["field_errors"]["consent"]
            .as_str()
            .unwrap_or_default()
            .contains("version"),
        "and says which version is current: {body}"
    );
    assert_eq!(
        harness.guests().await,
        0,
        "a refused guest request must not leave an account behind"
    );
    assert!(!reader.has_session(), "and must not hand out a session");

    // A version that is not the current one is refused the same way: consenting to different terms is not consenting to
    // these.
    let (status, body) = reader
        .post("/api/v1/auth/guest", json!({ "consent": { "version": 0 } }))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{status} {body}");
    assert_eq!(harness.guests().await, 0, "and still nothing is created");
}

#[tokio::test]
async fn a_guest_reads_before_registering_and_the_consent_is_recorded() {
    let harness = Harness::new("guest").await;
    let version = harness.config.consent.version;
    let mut reader = harness.client();

    let (status, body) = reader
        .post(
            "/api/v1/auth/guest",
            json!({ "consent": { "version": version, "categories": { "essential": true, "analytics": false } } }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{status} {body}");
    assert_eq!(body["account"]["is_guest"], json!(true), "the account view says what it is: {body}");
    assert!(
        reader.has_session(),
        "a guest reads with a session, not as an anonymous stranger"
    );

    // The record is a row, not a flag in a cookie.
    assert_eq!(harness.consent_rows().await, 1, "one acceptance, one row");
    let (status, me) = reader.get("/api/v1/auth/me").await;
    assert_eq!(status, StatusCode::OK, "{me}");
    assert_eq!(
        me["account"]["is_guest"], json!(true),
        "`/auth/me` reports it too, so a client can offer \"keep what you did\" when the guest registers: {me}"
    );

    // And the guest is a real account with a pseud: something a bookmark can belong to.
    let pseuds = darkhold_db::identity::pseuds_for_account(
        harness.db(),
        me["account"]["id"].as_str().expect("id").parse().expect("an account id"),
    )
    .await
    .expect("pseuds");
    assert_eq!(pseuds.len(), 1, "a guest has a pseud, which is what GS-3 upgrades in place");
}

#[tokio::test]
async fn asking_twice_keeps_the_same_guest() {
    let harness = Harness::new("twice").await;
    let version = harness.config.consent.version;
    let mut reader = harness.client();

    let (status, first) = reader
        .post("/api/v1/auth/guest", json!({ "consent": { "version": version } }))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{first}");
    let account = first["account"]["id"].as_str().expect("id").to_owned();

    let (status, second) = reader
        .post("/api/v1/auth/guest", json!({ "consent": { "version": version } }))
        .await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["created"], json!(false), "the same guest, not a second one: {second}");
    assert_eq!(
        harness.guests().await,
        1,
        "a page load is not a new identity: guest for {account} only"
    );
}

#[tokio::test]
async fn the_gate_is_a_switch_an_operator_owns() {
    // `required_for_guest = false` is the only thing that may skip the interstitial. This test exists so the switch has a
    // reader: a config field nobody reads is a setting that quietly does nothing.
    let mut config = config_for(&scratch_dir("switch"));
    config.consent.required_for_guest = false;
    let harness = Harness::with("switch", config).await;
    let mut reader = harness.client();

    let (status, body) = reader.post("/api/v1/auth/guest", json!({})).await;
    assert_eq!(status, StatusCode::CREATED, "{status} {body}");
    assert_eq!(body["account"]["is_guest"], json!(true));
    assert_eq!(
        harness.consent_rows().await,
        0,
        "nothing was accepted, so nothing is recorded — the row means \"they agreed\", not \"they passed through\""
    );
}

#[tokio::test]
async fn a_registered_reader_cannot_step_back_into_being_a_guest() {
    let harness = Harness::new("reverse").await;
    let version = harness.config.consent.version;
    let mut reader = harness.client();
    reader.register("reverse@example.test", "Reverse").await;

    let (status, body) = reader
        .post("/api/v1/auth/guest", json!({ "consent": { "version": version } }))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{status} {body}");
    assert!(
        body["error"]["field_errors"].get("session").is_some(),
        "the refusal names the session, not the consent: the consent was fine: {body}"
    );
    assert_eq!(harness.guests().await, 0, "no guest row was created");

    let (_, me) = reader.get("/api/v1/auth/me").await;
    assert_eq!(me["account"]["is_guest"], json!(false), "{me}");
}
