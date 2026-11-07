//! M69-06 — the rescue ring through its doors.
//!
//! The claim, in one line: a hunt reports who else has the media, and an ambiguity is left to the operator rather than
//! resolved by the ring.
//!
//! The media reference is created the way the instance creates one (a direct insert, because the fetch path needs a network
//! and this test is about the ring), then a survivor is recorded, then the hunt is run through `POST /operator/rescue/{peer}`.

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
        "darkhold-m6906-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
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

    fn swap(&mut self, app: axum::Router) {
        self.app = app;
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
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    async fn register(&mut self, handle: &str) -> String {
        let (status, body) = self
            .send(
                "POST",
                "/api/v1/auth/register",
                Some(json!({
                    "email": format!("m69-06-{handle}@example.test"),
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
        let (_, me) = self.send("GET", "/api/v1/auth/me", None).await;
        me["account"]["id"]
            .as_str()
            .or_else(|| me["id"].as_str())
            .expect("account id")
            .to_owned()
    }
}

fn router(tdb: &TestDb, dir: &std::path::Path, operator: Option<&str>, open: bool) -> axum::Router {
    let mut config = Config::development_defaults();
    config.storage.root = dir.to_path_buf();
    config.administration.operator_account_id = operator.map(|id| id.parse().expect("account id"));
    config.discovery.media_rescue = open;
    server::build_router(AppState::new(config, tdb.db().clone()))
}

/// A media reference as the fetch path would have left it.
async fn seed_reference(tdb: &TestDb, content_hash: &str, perceptual: Option<&str>) -> String {
    let sql = tdb.db().sql(
        "INSERT INTO media_references (id, perceptual_hash, content_hash, media_kind, first_seen_at, width, height)
         VALUES (?, ?, ?, 'image', '2026-11-01T00:00:00Z', 400, 300)",
        "INSERT INTO media_references (id, perceptual_hash, content_hash, media_kind, first_seen_at, width, height)
         VALUES ($1, $2, $3, 'image', '2026-11-01T00:00:00Z', 400, 300)",
    );
    let id = uuid::Uuid::new_v4().to_string();
    match tdb.db().backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(&id)
                .bind(perceptual)
                .bind(content_hash)
                .execute(tdb.db().sqlite_pool().expect("sqlite"))
                .await
                .expect("seed reference");
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(&sql)
                .bind(&id)
                .bind(perceptual)
                .bind(content_hash)
                .execute(tdb.db().postgres_pool().expect("postgres"))
                .await
                .expect("seed reference");
        }
    }
    id
}

#[tokio::test]
async fn a_hunt_finds_the_survivor_and_an_ambiguity_is_left_to_the_operator() {
    server::set_trust_proxy(false);
    let dir = scratch_dir("ring");
    let tdb = TestDb::connect_with_dir("m69-06-ring", &dir).await;

    let mut operator = Client::new(router(&tdb, &dir, None, true));
    let operator_id = operator.register("rescue-operator").await;
    operator.swap(router(&tdb, &dir, Some(operator_id.as_str()), true));

    // A survivor reports the bytes this instance lost.
    let (status, body) = operator
        .send(
            "POST",
            "/api/v1/operator/rescue/copies",
            Some(json!({
                "instance": "https://alive.example",
                "content_hash": "content-alive-0001",
                "perceptual_hash": "ffff0000ffff0000",
                "width": 400,
                "height": 300,
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["copy"]["instance"], "https://alive.example");

    let reference = seed_reference(&tdb, "content-alive-0001", Some("ffff0000ffff0000")).await;
    let (status, body) = operator
        .send(
            "POST",
            "/api/v1/operator/rescue",
            Some(json!({ "dead_peer": "https://dead.example", "media_reference_id": reference })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["hunt"]["outcome"], "exact", "{body}");
    assert_eq!(body["hunt"]["matched_instance"], "https://alive.example");
    let hunt_id = body["hunt"]["id"].as_str().expect("an id").to_owned();

    // A second survivor with the same bytes makes the next hunt ambiguous, and it names nobody.
    let (status, body) = operator
        .send(
            "POST",
            "/api/v1/operator/rescue/copies",
            Some(json!({
                "instance": "https://second.example",
                "content_hash": "content-alive-0001",
                "width": 400,
                "height": 300,
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = operator
        .send(
            "POST",
            "/api/v1/operator/rescue",
            Some(json!({ "dead_peer": "https://dead.example", "media_reference_id": reference })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["hunt"]["outcome"], "ambiguous", "{body}");
    assert!(body["hunt"]["matched_instance"].is_null(), "{body}");
    assert_eq!(
        body["hunt"]["candidates"].as_array().expect("candidates").len(),
        2,
        "{body}"
    );
    let ambiguous_id = body["hunt"]["id"].as_str().expect("id").to_owned();

    // Accepting the ambiguous one without a name is refused: the ring will not choose.
    let (status, body) = operator
        .send(
            "POST",
            &format!("/api/v1/operator/rescue/hunts/{ambiguous_id}/resolve"),
            Some(json!({ "resolution": "accepted" })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("instance"),
        "{body}"
    );

    // Naming one settles it.
    let (status, body) = operator
        .send(
            "POST",
            &format!("/api/v1/operator/rescue/hunts/{ambiguous_id}/resolve"),
            Some(json!({ "resolution": "accepted", "instance": "https://second.example" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["credited"], "https://second.example");

    // The exact hunt accepts without repeating the name the ring already reported.
    let (status, body) = operator
        .send(
            "POST",
            &format!("/api/v1/operator/rescue/hunts/{hunt_id}/resolve"),
            Some(json!({ "resolution": "accepted" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["credited"], "https://alive.example");

    let (status, body) = operator.send("GET", "/api/v1/operator/rescue", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["hunts"].as_array().expect("hunts").len(), 2);
    assert_eq!(body["open"], 0, "both were settled: {body}");
}

#[tokio::test]
async fn a_malformed_hash_is_refused_and_a_missing_reference_is_404() {
    server::set_trust_proxy(false);
    let dir = scratch_dir("bad");
    let tdb = TestDb::connect_with_dir("m69-06-bad", &dir).await;
    let mut operator = Client::new(router(&tdb, &dir, None, true));
    let operator_id = operator.register("rescue-operator2").await;
    operator.swap(router(&tdb, &dir, Some(operator_id.as_str()), true));

    let (status, body) = operator
        .send(
            "POST",
            "/api/v1/operator/rescue/copies",
            Some(json!({
                "instance": "https://alive.example",
                "content_hash": "content-alive-0001",
                "perceptual_hash": "not-a-hash",
            })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("perceptual hash"),
        "{body}"
    );

    let (status, body) = operator
        .send(
            "POST",
            "/api/v1/operator/rescue",
            Some(json!({ "dead_peer": "https://dead.example", "media_reference_id": "no-such-reference" })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

#[tokio::test]
async fn with_the_switch_off_every_door_is_404() {
    server::set_trust_proxy(false);
    let dir = scratch_dir("off");
    let tdb = TestDb::connect_with_dir("m69-06-off", &dir).await;
    let mut client = Client::new(router(&tdb, &dir, None, false));
    let account = client.register("rescue-closed").await;
    client.swap(router(&tdb, &dir, Some(account.as_str()), false));

    for (method, path) in [
        ("GET", "/api/v1/operator/rescue"),
        ("GET", "/api/v1/operator/rescue/copies"),
        ("POST", "/api/v1/operator/rescue/copies"),
        ("POST", "/api/v1/operator/rescue"),
        ("POST", "/api/v1/operator/rescue/hunts/x/resolve"),
    ] {
        let (status, body) = client
            .send(method, path, (method == "POST").then(|| json!({})))
            .await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "{method} {path} answered {status}: {body}"
        );
    }
}
