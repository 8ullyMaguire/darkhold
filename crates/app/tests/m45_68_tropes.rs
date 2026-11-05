//! M45-68 — a trope group is a name a reader can use, and the expansion is published.
//!
//! The AST is proven in `crates/domain/tests/trope_query.rs` (a group becomes an `Or` of `tag:`
//! leaves; an unknown name is refused with the list; an expansion spends the same budget as writing
//! the tags out). What this file proves is the join a reader actually touches: `/api/v1/tropes` says
//! what a group means, and `/api/v1/search?q=trope:…` accepts the name instead of answering `422`.

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

#[tokio::test]
async fn the_groups_are_published_with_the_tags_they_expand_to() {
    let harness = Harness::new("tropes-listed").await;
    let mut client = harness.client();

    let (status, body) = client.get("/api/v1/tropes").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let items = body["items"].as_array().expect("items");
    assert!(!items.is_empty(), "{body}");

    let dark = items
        .iter()
        .find(|item| item["slug"] == "dark-progression")
        .expect("the group the trope library leans on");
    assert_eq!(dark["label"], "Dark progression");
    assert!(
        dark["expands_to"]
            .as_array()
            .expect("expands_to")
            .iter()
            .any(|tag| tag == "dark-protagonist"),
        "the expansion is published, not implied: {dark}"
    );
    assert!(
        !dark["summary"].as_str().unwrap_or_default().is_empty(),
        "a group with no explanation is a name nobody can use: {dark}"
    );
}

#[tokio::test]
async fn the_search_accepts_the_name_instead_of_refusing_it() {
    let harness = Harness::new("tropes-search").await;
    let mut client = harness.client();

    // The catalogue is empty here, so the answer is an empty page — what matters is that the name
    // parsed and reached the engine, because a `422` is what a reader would otherwise get.
    let (status, body) = client
        .get("/api/v1/search?q=trope%3Asystem-grind&limit=5")
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // And an unknown group is refused *by name*, not answered with an empty page.
    let (status, body) = client
        .get("/api/v1/search?q=trope%3Anonsense&limit=5")
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body.to_string().contains("not a trope group"),
        "the refusal explains itself: {body}"
    );
}
