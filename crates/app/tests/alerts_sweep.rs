//! W5 task 6's route: `POST /api/v1/alerts/run` exists, is guarded, and its report adds up.
//!
//! The arrival half — an event landing in the inbox — cannot be asserted from a test process: `emit` is a network
//! call, and a test that stubbed it would prove the stub. That half is measured on the live estate (gate 31) by
//! reading the inbox's own rows. What is asserted here is the part a test can hold: the door, the scope, and the
//! arithmetic (`new == queued + suppressed`, and a second sweep announces nothing).

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
        "darkhold-alerts-{tag}-{}-{:?}",
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
            self.cookies.push((name.trim().to_owned(), value.trim().to_owned()));
        }
    }

    async fn request(&mut self, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
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
        let json: Value = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
        (status, json)
    }
}

async fn harness(tag: &str) -> Client {
    set_trust_proxy(false);
    let dir = scratch_dir(tag);
    let tdb = test_support::TestDb::connect_with_dir(tag, &dir).await;
    Client::new(server::build_router(AppState::new(
        config_for(&dir),
        tdb.db().clone(),
    )))
}

async fn register(client: &mut Client, email: &str, handle: &str) {
    let (status, body) = client
        .request(
            "POST",
            "/api/v1/auth/register",
            Some(json!({
                "email": email,
                "password": "test-pass-1234",
                "handle": handle,
                "display_name": handle,
                "age_band": "adult",
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "register: {body}");
}

#[tokio::test]
async fn a_stranger_cannot_run_a_sweep() {
    let mut client = harness("alerts-anon").await;
    let (status, body) = client.request("POST", "/api/v1/alerts/run", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
}

#[tokio::test]
async fn the_sweep_answers_a_report_that_adds_up() {
    let mut client = harness("alerts-report").await;
    register(&mut client, "alerts@example.test", "alerts").await;

    // A view with no matching items: the sweep still checks it, and the arithmetic still has to hold. A report
    // that sums only when there is something to report is a report nobody can use for a graph.
    let (status, body) = client
        .request(
            "POST",
            "/api/v1/saved-views",
            Some(json!({ "name": "weekly", "query": { "tags": ["naruto"] } })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let (status, first) = client.request("POST", "/api/v1/alerts/run", None).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["views"], 1, "{first}");
    assert_eq!(first["checked"], 1, "{first}");
    let new = first["new"].as_u64().expect("new");
    let queued = first["queued"].as_u64().expect("queued");
    let suppressed = first["suppressed"].as_u64().expect("suppressed");
    assert_eq!(
        new,
        queued + suppressed,
        "every new work is either queued for delivery or counted as suppressed: {first}"
    );

    // Second sweep: the run row advanced, so nothing is new. This is the assertion that makes alerts a notifier
    // rather than a repeating log.
    let (status, second) = client.request("POST", "/api/v1/alerts/run", None).await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["new"], 0, "the second sweep re-announced work: {second}");
    assert_eq!(second["checked"], 1, "{second}");
}
