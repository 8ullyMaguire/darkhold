//! M45-48 — what the machine is, what that buys, and what the operator said instead.
//!
//! The derivation itself is proven in `darkhold_domain::capacity` (six tests sitting on either side of
//! every threshold, including "eight cores on SQLite is `Small`"). What this file proves is the join:
//! the route publishes the facts it measured, and the tier it names is the one those facts buy —
//! re-derived here from the same pure function, so a route that hard-coded a tier would fail.

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::Database;
use serde_json::Value;
use test_support::TestDb;
use tower::ServiceExt;

struct Harness {
    _tdb: TestDb,
    _dir: std::path::PathBuf,
    config: Config,
    db: Database,
}

impl Harness {
    async fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "darkhold-m63-06-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        let mut config = Config::development_defaults();
        config.storage.root = dir.clone();
        let tdb = TestDb::connect_with_dir(tag, &dir).await;
        let db = tdb.db().clone();
        Self {
            _tdb: tdb,
            _dir: dir,
            config,
            db,
        }
    }

    fn client(&self) -> Client {
        Client {
            app: server::build_router(AppState::new(self.config.clone(), self.db.clone())),
            cookies: Vec::new(),
        }
    }
}

struct Client {
    app: axum::Router,
    cookies: Vec<(String, String)>,
}

impl Client {
    async fn request(
        &mut self,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder().method(method).uri(uri);
        let cookies = self
            .cookies
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; ");
        if !cookies.is_empty() {
            builder = builder.header(header::COOKIE, cookies);
        }
        if !matches!(method, "GET" | "HEAD" | "OPTIONS") {
            if let Some((_, token)) = self.cookies.iter().find(|(k, _)| k == "darkhold_csrf") {
                builder = builder.header("x-csrf-token", token.clone());
            }
        }
        let request = match body {
            Some(ref value) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(value).expect("serialise")))
                .expect("request"),
            None => builder.body(Body::empty()).expect("request"),
        };
        let response = self.app.clone().oneshot(request).await.expect("response");
        let status = response.status();
        for value in response.headers().get_all(header::SET_COOKIE) {
            let Ok(raw) = value.to_str() else { continue };
            let pair = raw.split(';').next().unwrap_or(raw);
            let Some((name, val)) = pair.split_once('=') else {
                continue;
            };
            self.cookies.retain(|(key, _)| key != name);
            if !val.is_empty() {
                self.cookies.push((name.to_owned(), val.to_owned()));
            }
        }
        let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
            .await
            .expect("body");
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes)
                .unwrap_or(Value::String(String::from_utf8_lossy(&bytes).into_owned()))
        };
        (status, value)
    }

    async fn get(&mut self, uri: &str) -> (StatusCode, Value) {
        self.request("GET", uri, None).await
    }
}

/// The measured facts, as the route publishes them.
fn measured(body: &Value) -> darkhold_domain::capacity::Capacity {
    let measured = &body["measured"];
    darkhold_domain::capacity::Capacity {
        cores: measured["cores"].as_u64().expect("cores") as usize,
        memory_mb: measured["memory_mb"].as_u64().expect("memory_mb"),
        engine: match measured["engine"].as_str().expect("engine") {
            "postgres" => darkhold_domain::capacity::Engine::Postgres,
            "sqlite" => darkhold_domain::capacity::Engine::Sqlite,
            other => panic!("unknown engine {other}"),
        },
        has_redis: measured["has_cache"].as_bool().expect("has_cache"),
    }
}

#[tokio::test]
async fn the_tier_it_names_is_the_one_its_own_facts_buy() {
    let harness = Harness::new("capacity-auto").await;
    let mut client = harness.client();

    let (status, body) = client.get("/api/v1/instance/capacity").await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let facts = measured(&body);
    let expected = darkhold_domain::capacity::Tier::of(&facts);
    assert_eq!(
        body["tier"].as_str().expect("tier"),
        expected.as_str(),
        "the route named a tier its own published facts do not buy: {body}"
    );
    assert_eq!(
        body["report"]["embeddings"].as_bool().expect("embeddings"),
        expected.report().embeddings,
        "the report must be the tier's report, not a second opinion: {body}"
    );
    assert_eq!(
        body["report"]["batch_size"].as_u64().expect("batch_size") as u32,
        expected.report().batch_size
    );
    assert!(
        body["override_tier"].is_null(),
        "with `tier = auto` there is no operator word to report: {body}"
    );

    // The facts are real: this test runs on a machine with at least one core and, on SQLite, says so.
    assert!(facts.cores >= 1, "{body}");
    assert_eq!(
        facts.engine,
        darkhold_domain::capacity::Engine::Sqlite,
        "the test harness runs SQLite, and the route must measure the engine rather than assume one: \
         {body}"
    );
}

#[tokio::test]
async fn an_operators_tier_answers_and_the_measurement_stays_visible() {
    let mut harness = Harness::new("capacity-override").await;
    harness.config.capacity.tier = "large".to_string();
    let mut client = harness.client();

    let (status, body) = client.get("/api/v1/instance/capacity").await;
    assert_eq!(status, StatusCode::OK, "{body}");

    assert_eq!(
        body["tier"].as_str().expect("tier"),
        "large",
        "the operator's word answers: {body}"
    );
    assert_eq!(
        body["override_tier"].as_str().expect("override"),
        "large",
        "and the response says the tier came from the operator, not from the box: {body}"
    );
    assert_eq!(
        body["report"]["embeddings"].as_bool(),
        Some(true),
        "large affords embeddings: {body}"
    );

    // The measurement is still published, so the disagreement is visible rather than implied.
    let facts = measured(&body);
    assert_eq!(
        facts.engine,
        darkhold_domain::capacity::Engine::Sqlite,
        "the box is still SQLite, whatever the operator asked for: {body}"
    );
    assert_eq!(
        body["measured"]["has_cache"].as_bool(),
        Some(false),
        "and no cache is configured: {body}"
    );
}
