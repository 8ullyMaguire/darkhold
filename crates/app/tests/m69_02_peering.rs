//! M69-02 — peering suggestions through the doors: two thresholds, one floor, and a veto that outranks both.
//!
//! The whole feature is a comparison between two documents, so the test drives the real pair: seed this instance's
//! published fingerprint (M69-01's door, made public), record three peers — a neighbour, a taste-twin that enforces a
//! different hardcoded layer, and a stranger — then read the operator's list and assert the verdicts *and their order*.
//!
//! The twin is the test's reason for existing. Without the veto it is the best candidate in the list, which is exactly why
//! asserting "the twin is blocked" is stronger than asserting "the neighbour is suggested": the second would pass on an
//! implementation with no veto at all.

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
        "darkhold-m6902-{tag}-{}-{:?}",
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
        Self { app, cookies: Vec::new() }
    }

    /// Swap the router under the same session, so a config change does not cost the cookies.
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
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.expect("body");
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    async fn register(&mut self, handle: &str) -> String {
        let (status, body) = self
            .send(
                "POST",
                "/api/v1/auth/register",
                Some(json!({
                    "email": format!("m69-02-{handle}@example.test"),
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
    config.discovery.peering_suggestions = open;
    // The declared hardcoded layer this instance publishes — the veto's other half.
    config.discovery.enforced_rules = vec!["CSAM".to_owned(), "minor-sexualisation".to_owned()];
    server::build_router(AppState::new(config, tdb.db().clone()))
}

fn peer_document(instance: &str, tags: &[(&str, f64)], enforced: &[&str]) -> String {
    let tags: Vec<Value> = tags
        .iter()
        .map(|(tag, weight)| json!({ "tag": tag, "weight": weight }))
        .collect();
    json!({
        "version": 1,
        "instance_id": instance,
        "generated_at": "2026-11-01T00:00:00Z",
        "tags": tags,
        "enforced": enforced,
    })
    .to_string()
}

/// This instance's own taste, written the way M69-01 reads it: a public `instance_themes` row.
async fn seed_own_taste(tdb: &TestDb, base_url: &str) {
    let vector = json!({
        "angst": 0.4,
        "fluff": 0.2,
        "au": 0.25,
        "hurt": 0.15,
    });
    darkhold_db::instance_theme::upsert_theme(
        tdb.db(),
        base_url,
        &vector,
        true,
        "2026-11-07T00:00:00Z",
    )
    .await
    .expect("seed instance theme");
}

#[tokio::test]
async fn the_list_suggests_the_neighbour_blocks_the_twin_and_ignores_the_stranger() {
    server::set_trust_proxy(false);
    let dir = scratch_dir("peering");
    let tdb = TestDb::connect_with_dir("m69-02-peering", &dir).await;

    let base_url = Config::development_defaults().site.base_url;
    seed_own_taste(&tdb, &base_url).await;

    let mut operator = Client::new(router(&tdb, &dir, None, false));
    let operator_id = operator.register("peering-operator").await;
    operator.swap(router(&tdb, &dir, Some(&operator_id), true));

    // The neighbour: same taste, same declared layer.
    let (status, body) = operator
        .send(
            "PUT",
            "/api/v1/operator/peering/fingerprints",
            Some(json!({
                "document": peer_document(
                    "https://near.example",
                    &[("angst", 0.38), ("fluff", 0.22), ("au", 0.24), ("hurt", 0.16)],
                    &["csam", "minor-sexualisation"],
                ),
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["enforced"], json!(["csam", "minor-sexualisation"]));

    // The twin: identical taste, enforces nothing. Without the veto this is the best candidate in the list.
    let (status, body) = operator
        .send(
            "PUT",
            "/api/v1/operator/peering/fingerprints",
            Some(json!({
                "document": peer_document(
                    "https://twin.example",
                    &[("angst", 0.4), ("fluff", 0.2), ("au", 0.25), ("hurt", 0.15)],
                    &[],
                ),
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // The stranger: different taste, same layer.
    let (status, body) = operator
        .send(
            "PUT",
            "/api/v1/operator/peering/fingerprints",
            Some(json!({
                "document": peer_document(
                    "https://stranger.example",
                    &[("omegaverse", 0.9), ("mpreg", 0.1)],
                    &["csam", "minor-sexualisation"],
                ),
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = operator
        .send("GET", "/api/v1/operator/peering/suggestions", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let candidates = body["candidates"].as_array().expect("candidates");
    assert_eq!(candidates.len(), 3, "{body}");

    // Order: suggestion, then the veto, then the near miss (see `peering::suggest`).
    assert_eq!(candidates[0]["instance_id"], json!("https://near.example"), "{body}");
    assert_eq!(candidates[0]["verdict"], json!("suggested"), "{body}");
    assert!(
        candidates[0]["similarity"].as_f64().expect("score") > 0.99,
        "a re-weighted copy of the same taste is a near-perfect match: {body}"
    );
    assert_eq!(candidates[1]["instance_id"], json!("https://twin.example"), "{body}");
    assert_eq!(
        candidates[1]["verdict"],
        json!("blocked"),
        "a taste twin with a different hardcoded layer is ineligible, not merely low: {body}"
    );
    assert!(
        candidates[1]["similarity"].is_null(),
        "a veto reports no score for anyone to act on: {body}"
    );
    assert!(
        candidates[1]["reason"].as_str().expect("reason").contains("csam"),
        "the reason names the rule: {body}"
    );
    assert_eq!(candidates[2]["instance_id"], json!("https://stranger.example"), "{body}");
    assert_eq!(candidates[2]["verdict"], json!("not_enough"), "{body}");
}

#[tokio::test]
async fn the_door_is_shut_unless_the_instance_opened_it_and_shut_means_404() {
    server::set_trust_proxy(false);
    let dir = scratch_dir("peering-closed");
    let tdb = TestDb::connect_with_dir("m69-02-closed", &dir).await;

    let base_url = Config::development_defaults().site.base_url;
    seed_own_taste(&tdb, &base_url).await;

    // `peering_suggestions` stays false: the default.
    let mut operator = Client::new(router(&tdb, &dir, None, false));
    let operator_id = operator.register("closed-operator").await;
    operator.swap(router(&tdb, &dir, Some(&operator_id), false));
    let (status, body) = operator
        .send("GET", "/api/v1/operator/peering/suggestions", None)
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "the list says who this instance would pair with; a default-on statement is one nobody chose: {body}"
    );

    let (status, _) = operator
        .send(
            "PUT",
            "/api/v1/operator/peering/fingerprints",
            Some(json!({ "document": peer_document("https://x.example", &[("angst", 1.0)], &[]) })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "recording is the same statement");
}

#[tokio::test]
async fn a_non_operator_is_refused_when_the_door_is_open() {
    server::set_trust_proxy(false);
    let dir = scratch_dir("peering-forbidden");
    let tdb = TestDb::connect_with_dir("m69-02-forbidden", &dir).await;
    let base_url = Config::development_defaults().site.base_url;
    seed_own_taste(&tdb, &base_url).await;

    let mut reader = Client::new(router(&tdb, &dir, None, true));
    let _ = reader.register("just-a-reader").await;
    let (status, body) = reader
        .send("GET", "/api/v1/operator/peering/suggestions", None)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
}
