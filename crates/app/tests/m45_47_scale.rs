//! M45-47: a mechanism declares a minimum activity, and is dormant below it.
//!
//! The row's claim is "small instances get simpler behaviour, features unlock as they grow". The
//! domain half is a declaration — `min_scale(id)` in one table, `Scale` with two facts — and this
//! is the half that decides whether a caller ever meets it:
//!
//! - **always reported**: `GET /api/v1/modules` publishes `observed_scale`, each row's `min_scale`
//!   and `dormant`, so "why is this page empty" has an answer with numbers in it;
//! - **optionally enforced**: with `[modules] scale_aware = true`, a dormant mechanism answers
//!   `404 MODULE_UNAVAILABLE`, the same refusal a switched-off module gives. Off by default,
//!   because refusing a surface that works is an operator's decision, not a default.
//!
//! `curator` is the fixture because it declares a real threshold (25 readers, 500 works) and a test
//! instance has one account and no works — the exact instance the row is about.

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
        "darkhold-m45-47-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn config_for(dir: &Path, scale_aware: bool) -> Config {
    let mut config = Config::development_defaults();
    config.storage.root = dir.to_path_buf();
    config.database = DatabaseConfig::new(format!(
        "sqlite://{}/darkhold.sqlite?mode=rwc",
        dir.display()
    ));
    config.modules.scale_aware = scale_aware;
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
        let json: Value = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
        (status, json)
    }

    async fn get(&mut self, uri: &str) -> (StatusCode, Value) {
        self.request("GET", uri, None).await
    }

    async fn register(&mut self) {
        let (status, body) = self
            .request(
                "POST",
                "/api/v1/auth/register",
                Some(json!({
                    "email": "scale@example.test",
                    "password": "test-pass-1234",
                    "handle": "scale",
                    "display_name": "scale",
                    "age_band": "adult",
                })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }
}

async fn router(tag: &str, scale_aware: bool) -> Client {
    set_trust_proxy(false);
    let dir = scratch_dir(tag);
    let tdb = test_support::TestDb::connect_with_dir(tag, &dir).await;
    let config = config_for(&dir, scale_aware);
    Client::new(server::build_router(AppState::new(
        config,
        tdb.db().clone(),
    )))
}

#[tokio::test]
async fn the_catalogue_publishes_the_scale_and_which_mechanisms_fall_short_of_it() {
    let mut client = router("reported", false).await;
    client.register().await;

    let (status, body) = client.get("/api/v1/modules").await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // One account, no works: `curator` asked for 25 and 500, so it is dormant.
    assert_eq!(body["observed_scale"]["active_readers"], json!(1), "{body}");
    assert_eq!(body["observed_scale"]["active_works"], json!(0), "{body}");
    assert_eq!(
        body["scale_aware"],
        json!(false),
        "reporting is the default"
    );

    let curator = body["modules"]
        .as_array()
        .expect("modules")
        .iter()
        .find(|row| row["id"] == "curator")
        .expect("curator is registered");
    assert_eq!(
        curator["min_scale"]["active_readers"],
        json!(25),
        "{curator}"
    );
    assert_eq!(
        curator["min_scale"]["active_works"],
        json!(500),
        "{curator}"
    );
    assert_eq!(curator["dormant"], json!(true), "{curator}");

    // A surface with no threshold is never dormant, whatever the instance size.
    let browse = body["modules"]
        .as_array()
        .expect("modules")
        .iter()
        .find(|row| row["id"] == "browse")
        .expect("browse is registered");
    assert_eq!(browse["dormant"], json!(false), "{browse}");
}

#[tokio::test]
async fn reporting_a_dormant_mechanism_does_not_take_it_away() {
    // The default. A reader on a small instance can still reach the surface; the
    // catalogue told them why it is empty, which is the whole complaint the row answers.
    let mut client = router("reported-not-refused", false).await;
    client.register().await;
    let (status, body) = client.get("/api/v1/curators").await;
    assert_ne!(
        status,
        StatusCode::NOT_FOUND,
        "reported dormancy must not become a 404 on a working surface: {body}"
    );
}

#[tokio::test]
async fn scale_aware_refuses_a_dormant_mechanism_with_the_same_404_as_switched_off() {
    let mut client = router("enforced", true).await;
    client.register().await;

    let (status, body) = client.get("/api/v1/curators").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(
        body["error"]["code"], "MODULE_UNAVAILABLE",
        "a caller that cannot use a surface is told the same thing whether it is off or early: {body}"
    );

    // And the catalogue agrees with the door rather than contradicting it.
    let (status, body) = client.get("/api/v1/modules").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["scale_aware"], json!(true), "{body}");
    let curator = body["modules"]
        .as_array()
        .expect("modules")
        .iter()
        .find(|row| row["id"] == "curator")
        .expect("curator");
    assert_eq!(curator["dormant"], json!(true), "{curator}");
}
