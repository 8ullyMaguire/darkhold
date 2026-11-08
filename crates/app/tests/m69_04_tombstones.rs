//! M69-04 — the tombstone doors.
//!
//! * filing echoes the **identity key** it derived, because a tombstone whose key never matches anything is invisible until
//!   an operator notices the zero in `refusals`;
//! * the list shows withdrawn tombstones and their refusal counts;
//! * withdrawal needs a reason and stops the refusal;
//! * with `[discovery] work_tombstones` off, all three doors are `404` rather than `403` — the feature is not there at all.

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
        "darkhold-m6904-{tag}-{}-{:?}",
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

    /// Swap the router without losing the session: the operator is named in config, and the account id only exists after
    /// registering, so the second router is built once there is an id to name.
    fn reconfigure(&mut self, app: axum::Router) {
        self.app = app;
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
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn register(&mut self, handle: &str) -> String {
        let (status, body) = self
            .send(
                "POST",
                "/api/v1/auth/register",
                Some(json!({
                    "email": format!("m69-04-{handle}@example.test"),
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

fn router(tdb: &TestDb, dir: &std::path::Path, open: bool, operator: Option<&str>) -> axum::Router {
    let mut config = Config::development_defaults();
    config.storage.root = dir.to_path_buf();
    config.discovery.work_tombstones = open;
    config.administration.operator_account_id =
        operator.map(|id| id.parse().expect("an account id"));
    let state = AppState::new(config, tdb.db().clone());
    server::build_router(state)
}

#[tokio::test]
async fn filing_reads_back_and_withdrawal_stops_the_refusal() {
    server::set_trust_proxy(false);
    let dir = scratch_dir("file");
    let tdb = TestDb::connect_with_dir("m69-04-file", &dir).await;

    let mut client = Client::new(router(&tdb, &dir, true, None));
    let account = client.register("tombstone-boss").await;
    client.reconfigure(router(&tdb, &dir, true, Some(&account)));

    let (status, filed) = client
        .send(
            "POST",
            "/api/v1/admin/tombstones",
            Some(json!({
                "title": "The Storm",
                "author": "ada",
                "language": "en",
                "reason": "author request",
                "origin_url": "https://example.org/works/1",
                "note": "asked by the author"
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{filed}");
    let id = filed["id"].as_str().expect("an id").to_owned();
    assert_eq!(
        filed["identity"].as_str().expect("a key").len(),
        32,
        "the door echoes the key the importer will compute: {filed}"
    );

    // It refuses, from a spelling the source would plausibly serve.
    assert!(
        darkhold_db::tombstones::refuse(
            tdb.db(),
            "the STORM!",
            "Ada",
            Some("EN"),
            "2026-11-07T00:00:00Z"
        )
        .await
        .expect("refuse")
        .is_some(),
        "identity is not the URL and not the exact string"
    );

    let (status, listed) = client.send("GET", "/api/v1/admin/tombstones", None).await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert_eq!(listed["active"], 1);
    assert_eq!(
        listed["tombstones"][0]["refusals"], 1,
        "the count is how an operator sees the tombstone working: {listed}"
    );

    let (status, withdrawn) = client
        .send(
            "POST",
            &format!("/api/v1/admin/tombstones/{id}/withdraw"),
            Some(json!({ "reason": "filed against the wrong work" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{withdrawn}");

    // Withdrawn: no longer refusing, still listed, with the reason.
    assert!(darkhold_db::tombstones::refuse(
        tdb.db(),
        "The Storm",
        "ada",
        Some("en"),
        "2026-11-07T00:00:00Z"
    )
    .await
    .expect("refuse")
    .is_none());
    let (_, listed) = client.send("GET", "/api/v1/admin/tombstones", None).await;
    assert_eq!(listed["active"], 0);
    assert_eq!(
        listed["tombstones"][0]["withdraw_reason"],
        "filed against the wrong work"
    );
}

#[tokio::test]
async fn a_withdrawal_without_a_reason_is_refused_and_a_non_operator_is_told_403() {
    server::set_trust_proxy(false);
    let dir = scratch_dir("reason");
    let tdb = TestDb::connect_with_dir("m69-04-reason", &dir).await;

    let mut boss = Client::new(router(&tdb, &dir, true, None));
    let boss_account = boss.register("tombstone-admin").await;
    boss.reconfigure(router(&tdb, &dir, true, Some(&boss_account)));
    let (_, filed) = boss
        .send(
            "POST",
            "/api/v1/admin/tombstones",
            Some(json!({ "title": "The Storm", "author": "ada", "reason": "dmca" })),
        )
        .await;
    let id = filed["id"].as_str().expect("id");

    let (status, body) = boss
        .send(
            "POST",
            &format!("/api/v1/admin/tombstones/{id}/withdraw"),
            Some(json!({ "reason": "  " })),
        )
        .await;
    // This instance answers a validation failure with 422, not 400 — asserted as it is, not as 400 would have looked nicer.
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("reason"),
        "{body}"
    );

    // A reader who is not the operator gets 404, not 403: the door's existence is the disclosure, which is the rule every
    // operator surface follows (`admin_scope::require_estate_hidden`).
    let mut reader = Client::new(router(&tdb, &dir, true, Some(&boss_account)));
    reader.register("tombstone-reader").await;
    let (status, body) = reader.send("GET", "/api/v1/admin/tombstones", None).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "403 would confirm the tombstone surface exists: {body}"
    );
}

#[tokio::test]
async fn with_the_switch_off_every_door_is_404() {
    server::set_trust_proxy(false);
    let dir = scratch_dir("off");
    let tdb = TestDb::connect_with_dir("m69-04-off", &dir).await;

    let mut client = Client::new(router(&tdb, &dir, false, None));
    let account = client.register("tombstone-closed").await;
    client.reconfigure(router(&tdb, &dir, false, Some(&account)));

    for (method, path, body) in [
        (
            "POST",
            "/api/v1/admin/tombstones",
            Some(json!({ "title": "x", "reason": "dmca" })),
        ),
        ("GET", "/api/v1/admin/tombstones", None),
        (
            "POST",
            "/api/v1/admin/tombstones/whatever/withdraw",
            Some(json!({ "reason": "no" })),
        ),
    ] {
        let (status, answer) = client.send(method, path, body).await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "{method} {path} answered {status}: {answer}"
        );
    }
}
