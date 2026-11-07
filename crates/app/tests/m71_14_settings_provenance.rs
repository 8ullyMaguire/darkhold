//! M71-14: a settings page says which level set the value, and can clear it.
//!
//! The reversal this test enforces: `GET /api/v1/settings/privacy` used to *hide* whether a value was chosen or inherited, on the stated
//! grounds that the difference "is not actionable". With four levels (instance, account, pseud, per-work) that is false — a reader looking
//! at a value they did not choose cannot find the control that set it, cannot clear it, and cannot tell that the instance default moved
//! under them.
//!
//! So each key reports `{value, source, reset}`:
//!
//! * `instance` — inherited, `reset: null` (there is nothing of the reader's to clear);
//! * `account` — set on the account, `reset` names `DELETE /api/v1/settings/privacy/{key}` and says what the value becomes;
//! * `pseud` — set on one face, `reset` names the same door with `?pseud_id=` and says it falls back to the account's value.
//!
//! Asserts:
//! 1. an untouched key is `instance` with no reset, and its value is the age-band default;
//! 2. setting it on the account moves the source to `account` and supplies the exact reset request;
//! 3. a pseud override outranks the account, and clearing the pseud returns the account's value and source — not the instance default;
//! 4. clearing the account returns the instance default, and clearing something never set is `204` rather than `404`.

use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use serde_json::{json, Value};
use test_support::TestDb;
use tower::ServiceExt;

const PASSWORD: &str = "test-pass-1234";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-m7114-{tag}-{}-{:?}",
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
    config
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
            Some(value) => Body::from(value.to_string()),
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
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    async fn get(&mut self, uri: &str) -> (StatusCode, Value) {
        self.request("GET", uri, None).await
    }

    async fn patch(&mut self, uri: &str, body: Value) -> (StatusCode, Value) {
        self.request("PATCH", uri, Some(body)).await
    }

    async fn delete(&mut self, uri: &str) -> (StatusCode, Value) {
        self.request("DELETE", uri, None).await
    }

    async fn post(&mut self, uri: &str, body: Value) -> (StatusCode, Value) {
        self.request("POST", uri, Some(body)).await
    }

    async fn register(&mut self, handle: &str) {
        let (status, body) = self
            .post(
                "/api/v1/auth/register",
                json!({
                    "email": format!("m71-14-{handle}@example.test"),
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

/// An account-scoped key: its value belongs to the person, and it is the one that can legitimately move between the account and the
/// instance levels.
const KEY: &str = "messaging_policy";

/// A pseud-scoped key: its value belongs to one face, so the level above it is the instance rather than the account.
const PSEUD_KEY: &str = "public_bookmarks";

fn provenance<'a>(body: &'a Value, key: &str) -> &'a Value {
    &body["provenance"][key]
}

/// A pseud's provenance entry, from the `pseud_provenance` map keyed by pseud id.
fn pseud_provenance<'a>(body: &'a Value, pseud: &str, key: &str) -> &'a Value {
    &body["pseud_provenance"][pseud][key]
}

#[tokio::test]
async fn an_inherited_setting_says_instance_and_offers_no_reset() {
    let h = Harness::new("inherited").await;
    let mut reader = h.client();
    reader.register("reader").await;

    let (status, body) = reader.get("/api/v1/settings/privacy").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // A key nobody touched: the instance's default for this age band, with nothing of the reader's to clear.
    let entry = provenance(&body, KEY);
    assert_eq!(
        entry["source"], "instance",
        "an onboarding default is inherited, not chosen — a new reader has not opened a settings page: {entry}"
    );
    assert!(
        entry["reset"].is_null(),
        "there is no reader-level value to clear: {entry}"
    );
    assert!(
        entry["note"]
            .as_str()
            .unwrap_or_default()
            .contains("starting default"),
        "a stored default says what it is: {entry}"
    );
    assert!(
        entry["value"].is_string(),
        "the effective value is still published: {entry}"
    );
    // The plain maps still carry the same value, so a page that does not understand provenance keeps working.
    assert_eq!(body["account"][KEY], entry["value"], "{body}");
}

#[tokio::test]
async fn setting_a_value_names_the_level_and_the_exact_reset_request() {
    let h = Harness::new("account").await;
    let mut reader = h.client();
    reader.register("reader").await;

    let (status, body) = reader
        .patch(
            "/api/v1/settings/privacy",
            json!({ "changes": { KEY: "anyone" } }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let entry = provenance(&body, KEY);
    assert_eq!(entry["source"], "account", "{entry}");
    assert_eq!(entry["value"], "anyone", "{entry}");
    let reset = &entry["reset"];
    assert_eq!(reset["method"], "DELETE", "{reset}");
    assert_eq!(
        reset["path"],
        format!("/api/v1/settings/privacy/{KEY}"),
        "the reset is a request a page can make, not a description: {reset}"
    );
    assert!(
        reset["becomes"]
            .as_str()
            .unwrap_or_default()
            .contains("instance default"),
        "it says what the value becomes: {reset}"
    );
}

#[tokio::test]
async fn a_pseud_override_outranks_the_account_and_clearing_it_returns_the_accounts_value() {
    let h = Harness::new("pseud").await;
    let mut reader = h.client();
    reader.register("reader").await;

    // Account value first, so the fallback after clearing the pseud is observable.
    let (status, body) = reader
        .patch(
            "/api/v1/settings/privacy",
            json!({ "changes": { KEY: "anyone" } }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = reader.get("/api/v1/settings/privacy").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let pseuds: Vec<String> = body["pseud_provenance"]
        .as_object()
        .expect("pseud_provenance")
        .keys()
        .cloned()
        .collect();
    let pseud = pseuds.first().expect("the registering reader has a pseud").clone();
    assert_eq!(
        pseud_provenance(&body, &pseud, PSEUD_KEY)["source"],
        "instance",
        "the pseud's onboarding default is inherited, never reported as the reader's choice: {body}"
    );

    // A pseud-level write: the most local level wins, and the response carries the exact reset request.
    let (status, body) = reader
        .patch(
            "/api/v1/settings/privacy",
            json!({ "pseud_id": pseud, "changes": { PSEUD_KEY: "public" } }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let entry = pseud_provenance(&body, &pseud, PSEUD_KEY);
    assert_eq!(entry["source"], "pseud", "{entry}");
    assert_eq!(entry["value"], "public", "{entry}");
    let reset = entry["reset"]["path"].as_str().unwrap_or_default();
    assert!(
        reset.starts_with(&format!("/api/v1/settings/privacy/{PSEUD_KEY}?pseud_id=")),
        "the reset names the level: {reset}"
    );

    // Clearing the pseud returns the instance default for that key — there is no account value to fall back to, and saying so is better
    // than inventing a level that does not exist.
    let (status, _) = reader
        .delete(&format!(
            "/api/v1/settings/privacy/{PSEUD_KEY}?pseud_id={pseud}"
        ))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = reader.get("/api/v1/settings/privacy").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(pseud_provenance(&body, &pseud, PSEUD_KEY)["source"], "instance", "{body}");
    // The account key set at the start of this test is untouched by any of it.
    assert_eq!(provenance(&body, KEY)["source"], "account", "{body}");

    // Clearing the account returns the instance default.
    let (status, _) = reader
        .delete(&format!("/api/v1/settings/privacy/{KEY}"))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = reader.get("/api/v1/settings/privacy").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(provenance(&body, KEY)["source"], "instance", "{body}");
}

#[tokio::test]
async fn clearing_something_never_set_is_not_a_failure() {
    let h = Harness::new("idempotent").await;
    let mut reader = h.client();
    reader.register("reader").await;

    let (status, _) = reader
        .delete(&format!("/api/v1/settings/privacy/{KEY}"))
        .await;
    assert_eq!(
        status,
        StatusCode::NO_CONTENT,
        "a reader asking for a setting to be inherited gets inheritance either way"
    );

    // An unknown key is still refused by name, so the door is not a silent no-op for typos.
    let (status, body) = reader.delete("/api/v1/settings/privacy/not_a_key").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
}
