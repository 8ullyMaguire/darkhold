//! The estate's error envelope, asserted on the wire (M72-04).
//!
//! Four products answer JSON errors in four shapes today: this one carries `request_id`, ascend carries `details`
//! but no id, kindred carries `code` and `message` only, ficnexus answers `{"err":-1,"msg":…}`. A client that has to
//! know which service it is talking to is a client with three parsers, and the estate probe that watches all of them
//! cannot read a status at all.
//!
//! The contract is the smallest thing that makes those parsers one:
//!
//! ```json
//! { "error": { "code": "…", "message": "…", "request_id": "…" } }
//! ```
//!
//! Extra fields are allowed (this product's `field_errors`, `conflict`, `required_scope`, `query` are how a form
//! explains itself), but these three are not optional, and they are what the probe reads.
//!
//! What this test is careful about: the classes below fail for *different reasons* — an unauthenticated request, an
//! unknown address, a validation refusal, a missing resource — and an envelope that only holds for one of them is an
//! envelope that holds for none. `request_id` in particular must be a real value on every refusal: "unavailable" is
//! what a request that never entered middleware would report, and a log line nobody can join is a log line nobody reads.

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
        "darkhold-envelope-{tag}-{}-{:?}",
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
}

/// Every refusal must carry the three fields, with a code the client can branch on and a message a person can read.
fn assert_envelope(label: &str, body: &Value) {
    let error = body
        .get("error")
        .unwrap_or_else(|| panic!("{label}: no `error` object in {body}"));

    let code = error
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{label}: no string `error.code` in {body}"));
    assert!(
        !code.is_empty(),
        "{label}: `error.code` is empty, so a client has nothing to branch on"
    );
    // Codes are contract names a client matches on: `UPPER_SNAKE`, never a sentence. The case is the
    // estate's existing convention (this product's `AUTH_REQUIRED`, kindred's `WORK_NOT_FOUND`), and the
    // point of writing it down is that a fourth product cannot quietly answer `notFound` or `err: -1`.
    assert!(
        code.chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'),
        "{label}: `error.code` is not UPPER_SNAKE: {code}"
    );

    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{label}: no string `error.message` in {body}"));
    assert!(!message.is_empty(), "{label}: `error.message` is empty");

    let request_id = error
        .get("request_id")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{label}: no string `error.request_id` in {body}"));
    assert!(
        !request_id.is_empty() && request_id != "unavailable",
        "{label}: `error.request_id` is {request_id:?} — a refusal nobody can join to a log line"
    );
}

#[tokio::test]
async fn every_refusal_carries_code_message_and_request_id() {
    let dir = scratch_dir("wire");
    let tdb = TestDb::connect_with_dir("envelope", &dir).await;
    let db: Database = tdb.db().clone();

    let mut config = Config::development_defaults();
    config.storage.root = dir.clone();
    let mut client = Client::new(server::build_router(AppState::new(
        config.clone(),
        db.clone(),
    )));

    // 1. Not authenticated. The most common refusal a client sees, and the one a probe hits while logged out.
    let (status, body) = client.send("GET", "/api/v1/library/items", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_envelope("unauthenticated", &body);

    // 2. Authenticated, and the address does not exist. An axum fallback, not a handler — a different code path that
    //    an envelope can easily miss.
    let (status, body) = client
        .send(
            "POST",
            "/api/v1/auth/register",
            Some(json!({
                "email": "envelope@example.test",
                "password": "test-pass-1234",
                "handle": "envelope",
                "display_name": "Envelope",
                "age_band": "adult",
            })),
        )
        .await;
    assert!(status.is_success(), "register: {body}");

    let (status, body) = client
        .send("GET", "/api/v1/no-such-collection", None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_envelope("unknown-route", &body);

    // 3. A validation refusal, where the body is the form's explanation.
    let (status, body) = client
        .send(
            "POST",
            "/api/v1/auth/register",
            Some(json!({ "email": "not-an-email", "password": "x" })),
        )
        .await;
    assert!(
        status.is_client_error(),
        "expected a refusal, got {status}: {body}"
    );
    assert_envelope(&format!("validation (status {status}, body {body})"), &body);

    // 4. A missing resource addressed by id — the shape a stale link produces, and the one where a service is most
    //    tempted to answer a bare 404 with no body at all.
    let (status, body) = client
        .send(
            "GET",
            "/api/v1/works/00000000-0000-4000-8000-000000000000",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_envelope("missing-resource", &body);

    // Two refusals must not report the same request id: it identifies a request, not a class of failure.
    // A fresh client, because the one above is signed in by now and an authenticated read is not a refusal.
    let mut anonymous = Client::new(server::build_router(AppState::new(
        config.clone(),
        db.clone(),
    )));
    let (_, first) = anonymous.send("GET", "/api/v1/library/items", None).await;
    let (_, second) = anonymous.send("GET", "/api/v1/library/items", None).await;
    let id = |body: &Value| body["error"]["request_id"].as_str().unwrap().to_owned();
    assert_ne!(
        id(&first),
        id(&second),
        "request ids are per request, or a log line joins every refusal"
    );
}
