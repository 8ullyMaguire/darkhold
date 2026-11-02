//! M47-08b — §9.6 private search history over HTTP.
//!
//! The store's own properties are proved in `crates/db/tests/search_history.rs`. This file proves
//! the three things only the route can get wrong:
//!
//! 1. **Anonymous is 401, not an empty list.** "No history" and "not signed in" must not look
//!    alike; the UI keys its whole rendering on the difference.
//! 2. **A successful search records; a rejected one does not.** Recording sits after
//!    `map_err(search_failure)?`, so a `422` from the query language leaves no entry — a query the
//!    parser refused is not one the reader would want offered back.
//! 3. **`search.record_history = false` stops both halves.** The list answers empty *and*
//!    `recording: false`, and a later search writes nothing, so the reader who turned it off is
//!    not shown a history that will never refill.
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
        "darkhold-m47-08-{tag}-{}-{:?}",
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

    /// One `Cookie` header with every cookie in it, the way a browser sends it: the server reads
    /// the first header only, so one-header-per-cookie would drop the session.
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

    async fn patch(&mut self, uri: &str, body: Value) -> (StatusCode, Value) {
        self.request("PATCH", uri, Some(body)).await
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
                    "email": format!("m47-08-{handle}@example.test"),
                    "password": PASSWORD,
                    "handle": handle,
                    "display_name": handle,
                    "age_band": "adult",
                }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "register {handle}: {body}");
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

/// The queries the history holds, newest first.
fn queries(body: &Value) -> Vec<String> {
    body["items"]
        .as_array()
        .expect("items is an array")
        .iter()
        .map(|item| item["query"].as_str().unwrap_or_default().to_owned())
        .collect()
}

#[tokio::test]
async fn an_anonymous_caller_is_401_not_an_empty_history() {
    let h = Harness::new("anon").await;
    let mut client = h.client();

    let (status, body) = client.get("/api/v1/search/history").await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "an anonymous history read must not look like an empty one: {body}"
    );

    let (status, _) = client.delete("/api/v1/search/history").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_search_is_recorded_with_the_count_it_returned() {
    let h = Harness::new("record").await;
    let mut client = h.client();
    client.register("reader").await;

    let (status, body) = client.get("/api/v1/search?q=anything").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let returned = body["items"].as_array().expect("items").len() as i64;

    let (status, body) = client.get("/api/v1/search/history").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["recording"].as_bool().unwrap_or(false));
    assert_eq!(queries(&body), vec!["anything".to_owned()]);
    assert_eq!(body["items"][0]["result_count"].as_i64(), Some(returned));
    assert!(
        body["items"][0]["searched_at"]
            .as_str()
            .is_some_and(|s| s.ends_with('Z')),
        "the timestamp is canonical UTC: {body}"
    );
}

#[tokio::test]
async fn a_rejected_query_is_not_recorded() {
    let h = Harness::new("reject").await;
    let mut client = h.client();
    client.register("reader").await;

    // An unterminated phrase is the parser's own error, and the parser's error is the 422
    // (`forum:` is a real field on the forum surface, so it is not an error on a works search).
    let (status, body) = client.get("/api/v1/search?q=%22unterminated").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    let (status, body) = client.get("/api/v1/search/history").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        queries(&body).is_empty(),
        "a 422 left an entry behind: {body}"
    );
}

#[tokio::test]
async fn clearing_empties_the_history_and_reports_the_count() {
    let h = Harness::new("clear").await;
    let mut client = h.client();
    client.register("reader").await;

    for q in ["first", "second"] {
        let (status, body) = client.get(&format!("/api/v1/search?q={q}")).await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    let (status, body) = client.delete("/api/v1/search/history").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["cleared"].as_i64(), Some(2));

    let (status, body) = client.get("/api/v1/search/history").await;
    assert_eq!(status, StatusCode::OK);
    assert!(queries(&body).is_empty(), "{body}");
}

#[tokio::test]
async fn turning_recording_off_stops_both_the_list_and_the_writes() {
    let h = Harness::new("off").await;
    let mut client = h.client();
    client.register("reader").await;

    let (status, body) = client.get("/api/v1/search?q=before").await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = client
        .patch(
            "/api/v1/settings/search",
            json!({ "changes": [{ "key": "search.record_history", "value": false }] }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "the key is recognized: {body}");

    let (status, body) = client.get("/api/v1/search/history").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["recording"].as_bool(), Some(false));
    // What was recorded before the switch is still shown, because it is still stored: hiding it
    // while keeping it would be the one combination a privacy switch must not produce. The reader
    // removes it with the Clear control.
    assert_eq!(queries(&body), vec!["before".to_owned()], "{body}");

    let (status, body) = client.get("/api/v1/search?q=after").await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // Nothing was written: turn recording back on and the list is still empty.
    let (status, body) = client
        .patch(
            "/api/v1/settings/search",
            json!({ "changes": [{ "key": "search.record_history", "value": true }] }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = client.get("/api/v1/search/history").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["recording"].as_bool(), Some(true));
    assert_eq!(
        queries(&body),
        vec!["before".to_owned()],
        "a search made while recording was off was stored anyway: {body}"
    );
}

#[tokio::test]
async fn one_reader_cannot_see_another_history() {
    let h = Harness::new("isolation").await;
    let mut alice = h.client();
    alice.register("alice").await;
    let mut bob = h.client();
    bob.register("bob").await;

    let (status, body) = alice.get("/api/v1/search?q=alices-secret-query").await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = bob.get("/api/v1/search/history").await;
    assert_eq!(status, StatusCode::OK);
    assert!(queries(&body).is_empty(), "{body}");

    // And a signed-out client sees 401 rather than alice's list.
    let mut anon = alice.signed_out();
    let (status, _) = anon.get("/api/v1/search/history").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
