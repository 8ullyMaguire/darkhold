//! M16-06 through the door: `POST /api/v1/operator/sandbox/run`.
//!
//! The sandbox crate tests the ceilings. This file tests the two things a crate test cannot: that the door exists and is **operator-only**
//! (running untrusted code must never be one session away from a reader), and that the ceilings a caller sends are the ceilings that run —
//! because M16-05's whole design is that a manifest declares its limits and something else enforces them.
//!
//! Asserts:
//! 1. a reader is refused and nothing runs;
//! 2. an operator gets a real outcome: output, host calls, fuel spent;
//! 3. a module that burns fuel is reported as `fuel`, not as a server error;
//! 4. a request with `fuel: 0` is refused by name rather than run under an invented default.

use std::path::PathBuf;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use base64::Engine as _;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use serde_json::{json, Value};
use test_support::TestDb;
use tower::ServiceExt;

const PASSWORD: &str = "test-pass-1234";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-m1606-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

struct Harness {
    _dir: PathBuf,
    tdb: TestDb,
    config: Config,
}

impl Harness {
    async fn new(tag: &str) -> Self {
        server::set_trust_proxy(false);
        let dir = scratch_dir(tag);
        let tdb = TestDb::connect_with_dir(tag, &dir).await;
        let mut config = Config::development_defaults();
        config.storage.root = dir.clone();
        Self {
            _dir: dir,
            tdb,
            config,
        }
    }

    /// The operator's door needs the *estate* claim, not TL5: `require_operator` is one decision (`estate_admin_allowed`), and a
    /// configured operator account is how a single-instance deployment names its operator. Setting it and rebuilding the router is
    /// closer to the deployment than inventing a role row.
    fn operator_account(&mut self, account: &str) {
        self.config.administration.operator_account_id =
            Some(account.parse().expect("account id parses"));
    }

    fn client(&self) -> Client {
        Client::new(server::build_router(AppState::new(
            self.config.clone(),
            self.tdb.db().clone(),
        )))
    }
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

    /// Swap the router while keeping the session cookies — the harness raises the operator *after* registering, and re-registering
    /// would be a different account.
    fn rebuild(&mut self, state: AppState) {
        self.app = server::build_router(state);
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

    async fn post(&mut self, uri: &str, body: Value) -> (StatusCode, Value) {
        let mut builder = Request::builder()
            .method("POST")
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json");
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
        let response = self
            .app
            .clone()
            .oneshot(builder.body(Body::from(body.to_string())).expect("request"))
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

    /// Register a reader and return their account id, so the caller can raise their trust.
    async fn register(&mut self, handle: &str) -> String {
        let (status, body) = self
            .post(
                "/api/v1/auth/register",
                json!({
                    "email": format!("m16-06-{handle}@example.test"),
                    "password": PASSWORD,
                    "handle": handle,
                    "display_name": handle,
                    "age_band": "adult",
                }),
            )
            .await;
        assert!(
            status == StatusCode::CREATED || status == StatusCode::OK,
            "register {handle}: {body}"
        );
        body["account"]["id"]
            .as_str()
            .or_else(|| body["id"].as_str())
            .unwrap_or_else(|| panic!("no account id in {body}"))
            .to_owned()
    }
}

fn base64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// A module that writes `count` bytes through the host import and returns 7.
fn writer(count: i32) -> String {
    let wasm = wat::parse_str(format!(
        r#"
        (module
          (import "darkhold" "write" (func $write (param i32 i32) (result i32)))
          (memory (export "memory") 1)
          (func (export "run") (param i32 i32) (result i32)
            (drop (call $write (i32.const 0) (i32.const {count})))
            (i32.const 7)))
        "#
    ))
    .expect("fixture compiles");
    base64(&wasm)
}

/// A module that loops without calling the host: only fuel can stop it.
fn spinner() -> String {
    let wasm = wat::parse_str(
        r#"
        (module
          (memory (export "memory") 1)
          (func (export "run") (param i32 i32) (result i32)
            (local $i i64)
            (loop $forever
              (local.set $i (i64.add (local.get $i) (i64.const 1)))
              (br $forever))
            (i32.const 0)))
        "#,
    )
    .expect("fixture compiles");
    base64(&wasm)
}

fn request_body(wasm: String) -> Value {
    json!({
        "wasm_base64": wasm,
        "entry": "run",
        "input_base64": "",
        "memory_mib": 1,
        "fuel": 10_000_000_u64,
        "host_calls": 10_u32,
        "output_bytes": 1024_usize,
        "wall_clock_ms": 2_000_u64,
        "apis": ["write", "checkpoint"],
    })
}

#[tokio::test]
async fn a_reader_cannot_run_code() {
    let h = Harness::new("reader").await;
    let mut client = h.client();
    client.register("reader").await;

    let (status, body) = client
        .post("/api/v1/operator/sandbox/run", request_body(writer(8)))
        .await;
    // `404`, not `403`: an operator door's existence is itself the disclosure (ADR 0030), so a reader learns only that there
    // is nothing at this path -- not that the instance can run code on their behalf.
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "running untrusted code is not a reader's verb, and 403 would say the sandbox exists: {body}"
    );
    assert!(
        body["output_base64"].is_null(),
        "and nothing was run on the way to the refusal: {body}"
    );
}

#[tokio::test]
async fn an_operator_runs_a_module_under_the_ceilings_the_request_names() {
    let mut h = Harness::new("operator").await;
    let mut client = h.client();
    let account = client.register("operator").await;
    h.operator_account(&account);
    client.rebuild(AppState::new(h.config.clone(), h.tdb.db().clone()));

    let (status, body) = client
        .post("/api/v1/operator/sandbox/run", request_body(writer(8)))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["succeeded"], true, "{body}");
    assert!(body["halt"].is_null(), "{body}");
    assert_eq!(body["host_calls"], 1, "{body}");
    assert_eq!(body["output_bytes"], 8, "{body}");
    assert_eq!(body["returned"], 7, "{body}");
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(body["output_base64"].as_str().expect("output"))
        .expect("valid base64");
    assert_eq!(
        decoded.len(),
        8,
        "the bytes the module wrote come back: {body}"
    );
    assert!(
        body["fuel_used"].as_u64().unwrap_or(0) > 0,
        "fuel metering is on in the door, not only in the library: {body}"
    );
}

#[tokio::test]
async fn a_run_that_hits_a_ceiling_reports_which_one() {
    let mut h = Harness::new("ceiling").await;
    let mut client = h.client();
    let account = client.register("operator").await;
    h.operator_account(&account);
    client.rebuild(AppState::new(h.config.clone(), h.tdb.db().clone()));

    let mut body = request_body(spinner());
    body["fuel"] = json!(40_000);
    let (status, body) = client.post("/api/v1/operator/sandbox/run", body).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a ceiling is a result, not an error: {body}"
    );
    assert_eq!(body["halt"], "fuel", "{body}");
    assert_eq!(body["succeeded"], false, "{body}");
}

#[tokio::test]
async fn a_request_with_no_fuel_is_refused_by_name() {
    let mut h = Harness::new("nofuel").await;
    let mut client = h.client();
    let account = client.register("operator").await;
    h.operator_account(&account);
    client.rebuild(AppState::new(h.config.clone(), h.tdb.db().clone()));

    let mut body = request_body(writer(4));
    body["fuel"] = json!(0);
    let (status, response) = client.post("/api/v1/operator/sandbox/run", body).await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "zero fuel is a request that cannot run, and saying so beats inventing a default: {response}"
    );
}

#[tokio::test]
async fn the_concurrency_bound_is_reachable_and_refuses_with_a_reason() {
    // The bound is what stops N submissions becoming N threads. Held here through the same state the door reads: with every slot taken, a
    // request is refused **without being run**, and the refusal carries a retry hint rather than looking like a module failure.
    let mut h = Harness::new("slots").await;
    let mut client = h.client();
    let account = client.register("operator").await;
    h.operator_account(&account);

    // One state for the client and for the assertions, so the count being observed is the count being enforced.
    let state = AppState::new(h.config.clone(), h.tdb.db().clone());
    client.rebuild(state.clone());

    let slots = state.sandbox_slots();
    let mut held = Vec::new();
    while let Some(slot) = slots.try_enter() {
        held.push(slot);
    }
    assert_eq!(
        held.len(),
        darkhold_app::routes::sandbox::MAX_CONCURRENT_RUNS,
        "the whole bound is holdable, so the refusal below is the bound and not a leak"
    );

    let (status, body) = client
        .post("/api/v1/operator/sandbox/run", request_body(writer(4)))
        .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert!(
        body["output_base64"].is_null(),
        "a refused run did not execute: {body}"
    );

    drop(held);
    let (status, body) = client
        .post("/api/v1/operator/sandbox/run", request_body(writer(4)))
        .await;
    assert_eq!(status, StatusCode::OK, "the slots are free again: {body}");
}
