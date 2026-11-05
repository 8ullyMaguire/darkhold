//! M63-13: a saved query can be run by a timer, which means by a token.
//!
//! `M63-06` made `/saved-views` a `RequireSession` surface and measured that a bearer token gets
//! `401` there — right for a browser-driven tool, wrong for anything scheduled, because a session
//! cookie expires in days and a timer has to run for months.
//!
//! The widening is deliberately **three verbs wide**: list, get, and record-a-run. Those are what an
//! unattended digest needs, and `library.read` covers them. Creating, renaming and deleting a view
//! stay session-only: a token minted to read a view every six hours has no business deleting it, and
//! a scope that would let it (`library.write`) does not exist yet — a row that needs bot-authored
//! views can add it, with this test as the precedent for what "widen one verb" looks like.

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
        "darkhold-m63-13-{tag}-{}-{:?}",
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

const PASSWORD: &str = "test-pass-1234";

struct Client {
    app: axum::Router,
    cookies: Vec<(String, String)>,
    token: Option<String>,
}

impl Client {
    fn new(app: axum::Router) -> Self {
        Self {
            app,
            cookies: Vec::new(),
            token: None,
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
        if let Some(token) = self.token.clone() {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        } else {
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
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&body).into_owned()));
        (status, json)
    }

    async fn get(&mut self, uri: &str) -> (StatusCode, Value) {
        self.request("GET", uri, None).await
    }

    async fn post(&mut self, uri: &str, body: Value) -> (StatusCode, Value) {
        self.request("POST", uri, Some(body)).await
    }

    async fn delete(&mut self, uri: &str) -> (StatusCode, Value) {
        self.request("DELETE", uri, None).await
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
        let (status, me) = self.get("/api/v1/auth/me").await;
        assert_eq!(status, StatusCode::OK, "{me}");
        me["pseuds"][0]["id"].as_str().expect("pseud id").to_owned()
    }

    async fn issue_token(&mut self, pseud_id: &str, scopes: &[&str]) -> (StatusCode, Value) {
        self.post(
            "/api/v1/me/tokens",
            json!({
                "name": "a timer token",
                "kind": "api",
                "scopes": scopes,
                "acting_pseud_id": pseud_id,
            }),
        )
        .await
    }
}

async fn harness(tag: &str) -> Client {
    set_trust_proxy(false);
    let dir = scratch_dir(tag);
    let tdb = test_support::TestDb::connect_with_dir(tag, &dir).await;
    let config = config_for(&dir);
    Client::new(server::build_router(AppState::new(
        config,
        tdb.db().clone(),
    )))
}

/// Create one saved view as the session, and return its id.
async fn a_saved_view(client: &mut Client) -> String {
    let (status, body) = client
        .post(
            "/api/v1/saved-views",
            json!({ "name": "weekly", "query": { "tags": ["naruto"] } }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_str().expect("view id").to_owned()
}

#[tokio::test]
async fn a_token_with_library_read_lists_and_reads_saved_views() {
    let mut session = harness("token-reads").await;
    let pseud = session.register("timer@example.test", "timer").await;
    let view = a_saved_view(&mut session).await;

    let (status, body) = session.issue_token(&pseud, &["library.read"]).await;
    assert!(
        status == StatusCode::CREATED || status == StatusCode::OK,
        "issue a library.read token: {status} {body}"
    );
    let secret = body["token"]
        .as_str()
        .or_else(|| body["secret"].as_str())
        .expect("the token is returned once")
        .to_owned();

    // The same app, a token instead of a cookie — the session's cookies are dropped so there is
    // no ambiguity about which credential answered.
    let mut tokenised = Client::new(session.app.clone());
    tokenised.token = Some(secret);

    let (status, body) = tokenised.get("/api/v1/saved-views").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a token with library.read lists: {body}"
    );
    assert_eq!(body["items"][0]["id"], json!(view));

    let (status, body) = tokenised.get(&format!("/api/v1/saved-views/{view}")).await;
    assert_eq!(status, StatusCode::OK, "and reads one: {body}");

    // Recording a run is bookkeeping for a read, so `library.read` covers it. Without this, every
    // scheduled `--digest-diff` would need a write scope it has no other use for.
    let (status, body) = tokenised
        .post(
            &format!("/api/v1/saved-views/{view}/runs"),
            json!({ "ids": ["work-1", "work-2"] }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "and records what it saw: {body}");
}

#[tokio::test]
async fn a_token_without_library_read_is_refused_by_naming_the_scope() {
    let mut session = harness("token-refused").await;
    let pseud = session.register("narrow@example.test", "narrow").await;
    a_saved_view(&mut session).await;

    let (status, body) = session.issue_token(&pseud, &["content.read"]).await;
    assert!(
        status == StatusCode::CREATED || status == StatusCode::OK,
        "issue a content.read token: {status} {body}"
    );
    let secret = body["token"]
        .as_str()
        .or_else(|| body["secret"].as_str())
        .expect("the token is returned once")
        .to_owned();

    let mut tokenised = Client::new(session.app.clone());
    tokenised.token = Some(secret);

    let (status, body) = tokenised.get("/api/v1/saved-views").await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(
        body["error"]["required_scope"], "library.read",
        "the refusal names the scope instead of making the caller guess: {body}"
    );
}

#[tokio::test]
async fn the_verbs_that_change_a_view_still_need_a_session() {
    let mut session = harness("token-mutates").await;
    let pseud = session.register("writer@example.test", "writer").await;
    let view = a_saved_view(&mut session).await;

    let (status, body) = session.issue_token(&pseud, &["library.read"]).await;
    assert!(
        status == StatusCode::CREATED || status == StatusCode::OK,
        "issue a library.read token: {status} {body}"
    );
    let secret = body["token"]
        .as_str()
        .or_else(|| body["secret"].as_str())
        .expect("the token is returned once")
        .to_owned();

    let mut tokenised = Client::new(session.app.clone());
    tokenised.token = Some(secret);

    // A timer's token may read the query it runs; it may not delete it. `RequireSession` is what
    // makes that true, and a token presenting itself is answered with 401, not 403 — it is the
    // wrong kind of credential for this door, not a credential with too few scopes.
    let (status, body) = tokenised
        .delete(&format!("/api/v1/saved-views/{view}"))
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
}
