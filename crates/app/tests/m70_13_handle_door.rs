//! The operator door that names a feed handle (§23.4).
//!
//! A handle is a public *name*: whatever the instance says at `/feeds/{name}` is whatever the operator pointed there. Without a
//! door the table had a writer and no caller, and every handle in a test had to be seeded by hand.
//!
//! Rules proven here:
//!
//! * naming is the operator's, and a reader who is not the operator is refused the shape ADR 0030 prescribes (`403` — the
//!   standing is not secret, the permission is absent);
//! * a handle is a URL path segment, so the characters are constrained at the door rather than discovered by a reader;
//! * a kind the feed cannot serve is refused **at naming time**, which is the only moment anybody can still choose differently;
//! * naming an existing handle re-points it instead of creating a second feed at one address.

use std::path::PathBuf;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::Database;
use serde_json::{json, Value};
use test_support::TestDb;
use tower::ServiceExt;

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-handle-door-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

struct Client {
    app: Router,
    cookies: Vec<(String, String)>,
}

impl Client {
    fn new(app: Router) -> Self {
        Self {
            app,
            cookies: Vec::new(),
        }
    }

    async fn send(&mut self, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut builder = Request::builder()
            .method(Method::from_bytes(method.as_bytes()).expect("method"))
            .uri(uri);
        if !self.cookies.is_empty() {
            let cookie = self
                .cookies
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect::<Vec<String>>()
                .join("; ");
            builder = builder.header(header::COOKIE, cookie);
        }
        if let Some((_, token)) = self.cookies.iter().find(|(k, _)| k == "darkhold_csrf") {
            builder = builder.header("x-csrf-token", token.clone());
        }
        let body = match body {
            Some(value) => {
                builder = builder.header(header::CONTENT_TYPE, "application/json");
                Body::from(value.to_string())
            }
            None => Body::empty(),
        };
        let response = self
            .app
            .clone()
            .oneshot(builder.body(body).expect("request"))
            .await
            .expect("response");
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
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
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
                    "email": format!("handledoor-{handle}@example.test"),
                    "password": "test-pass-1234",
                    "handle": handle,
                    "display_name": handle,
                    "age_band": "adult",
                })),
            )
            .await;
        assert!(status.is_success(), "register {handle}: {body}");
        let (_, me) = self.send("GET", "/api/v1/auth/me", None).await;
        me["account"]["id"]
            .as_str()
            .or_else(|| me["id"].as_str())
            .expect("account id")
            .to_owned()
    }
}

#[tokio::test]
async fn only_the_operator_names_a_handle_and_the_name_is_checked_here() {
    let dir = scratch_dir("name");
    let tdb = TestDb::connect_with_dir("handledoor", &dir).await;
    let db: Database = tdb.db().clone();

    let mut config = Config::development_defaults();
    config.storage.root = dir.clone();
    let mut first = Client::new(server::build_router(AppState::new(
        config.clone(),
        db.clone(),
    )));
    let operator_account = first.register("operator").await;

    let mut operator_config = config.clone();
    operator_config.administration.operator_account_id =
        Some(operator_account.parse().expect("account id"));
    let mut operator = Client::new(server::build_router(AppState::new(
        operator_config,
        db.clone(),
    )));
    // Carry the session across: the operator is the same account, seen through a build that knows it is the operator.
    operator.cookies = first.cookies.clone();

    // A plain reader has no operator standing at all, so the refusal is `404` and not `403` (ADR 0030: to a caller who is not
    // the operator the door does not exist — a `403` would confirm there is a door to want).
    let mut reader = Client::new(server::build_router(AppState::new(
        config.clone(),
        db.clone(),
    )));
    reader.register("plainreader").await;
    let (status, body) = reader
        .send(
            "PUT",
            "/api/v1/admin/feeds/naruto",
            Some(json!({ "kind": "tag", "subject": "naruto" })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // The operator names it, and the answer carries the address.
    let (status, body) = operator
        .send(
            "PUT",
            "/api/v1/admin/feeds/naruto",
            Some(json!({ "kind": "tag", "subject": "naruto" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["handle"], json!("naruto"), "{body}");
    assert!(
        body["url"]
            .as_str()
            .unwrap_or_default()
            .ends_with("/api/v1/feeds/naruto"),
        "{body}"
    );

    // The named handle serves.
    let (status, _) = reader.send("GET", "/api/v1/feeds/naruto", None).await;
    assert_eq!(status, StatusCode::OK);

    // Re-pointing is an update, not a second feed at one address.
    let (status, body) = operator
        .send(
            "PUT",
            "/api/v1/admin/feeds/naruto",
            Some(json!({ "kind": "user", "subject": "someone-else" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["kind"], json!("user"), "{body}");

    // Refusals: a kind nothing can serve, a handle that cannot go in a path, an empty subject.
    for (handle, payload, because) in [
        (
            "seriesfeed",
            json!({ "kind": "series", "subject": "abc" }),
            "a series kind",
        ),
        (
            // Percent-encoded, so the request is well-formed and the *decoded* handle is what the door must refuse.
            "has%20space",
            json!({ "kind": "tag", "subject": "abc" }),
            "a space in the path",
        ),
        (
            "emptysubject",
            json!({ "kind": "tag", "subject": "  " }),
            "no subject",
        ),
    ] {
        let (status, body) = operator
            .send(
                "PUT",
                &format!("/api/v1/admin/feeds/{handle}"),
                Some(payload),
            )
            .await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{because}: {body}"
        );
    }
}
