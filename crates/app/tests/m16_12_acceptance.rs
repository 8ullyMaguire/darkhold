//! M16-12 — §21's acceptance list, as tests rather than as sentences.
//!
//! The row exists because these are the claims that make the rest of §21 trustworthy. Each test below is one claim, driven through the
//! path a deployment actually uses (the operator door for the executor claims, the store for the grants claims, the product routes for the
//! theme claim), and each names the claim it is standing for so a failure reads as the promise that broke.
//!
//! | claim | here |
//! |---|---|
//! | infinite loops terminate | `an_infinite_loop_terminates` |
//! | memory excess fails safely | `memory_excess_fails_safely` |
//! | host APIs enforce permission denial | `a_host_api_refuses_a_package_without_the_grant` |
//! | uninstall removes grants | `uninstalling_a_grant_removes_it_and_reinstalling_restores_it` |
//! | reset remains reachable after a broken theme | `reset_is_reachable_after_a_broken_theme` |
//!
//! `revoked packages stop running` is the sandbox crate's `revocation_stops_a_running_module_at_its_next_host_call` — the flag is polled
//! by the host imports, and there is no door here that revokes a run in flight yet (that is M16-07's emergency path). The two claims not
//! covered at all are named in the ledger row: forks keep attribution and request their own grants, and custom engines and recipes honour
//! opt-out.

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
        "darkhold-m1612-{tag}-{}-{:?}",
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

    fn client(&self) -> Client {
        Client::new(server::build_router(AppState::new(
            self.config.clone(),
            self.tdb.db().clone(),
        )))
    }

    /// Register a reader, raise them to the estate operator, and hand back the **same** client so the session cookies survive: a fresh
    /// client is a fresh cookie jar, and the door would then answer 401 — which is what happened the first time this harness was written.
    async fn operator(&mut self, handle: &str) -> Client {
        let mut client = self.client();
        let account = client.register(handle).await;
        self.config.administration.operator_account_id = Some(account.parse().expect("account id"));
        client.rebuild(AppState::new(self.config.clone(), self.tdb.db().clone()));
        client
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

    /// Swap the router, keeping the cookies.
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

    async fn send(&mut self, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut builder = Request::builder().method(method).uri(uri);
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
                    "email": format!("m16-12-{handle}@example.test"),
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
        body["account"]["id"]
            .as_str()
            .or_else(|| body["id"].as_str())
            .unwrap_or_else(|| panic!("no account id in {body}"))
            .to_owned()
    }
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn body_for(wasm: &[u8], fuel: u64) -> Value {
    json!({
        "wasm_base64": b64(wasm),
        "entry": "run",
        "input_base64": "",
        "memory_mib": 1,
        "fuel": fuel,
        "host_calls": 10_u32,
        "output_bytes": 1024_usize,
        "wall_clock_ms": 2_000_u64,
        "apis": ["write", "checkpoint"],
    })
}

#[tokio::test]
async fn an_infinite_loop_terminates() {
    let mut h = Harness::new("loop").await;
    let mut client = h.operator("loop").await;

    // No imports at all: fuel is the only ceiling that can stop this, which is the claim in the row.
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

    let started = std::time::Instant::now();
    let (status, body) = client
        .send(
            "POST",
            "/api/v1/operator/sandbox/run",
            Some(body_for(&wasm, 60_000)),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["halt"], "fuel", "the loop ended on fuel: {body}");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "and it ended in seconds, not never: {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn memory_excess_fails_safely() {
    let mut h = Harness::new("memory").await;
    let mut client = h.operator("memory").await;

    // Asks for 32 MiB against a 1 MiB ceiling, then writes at the far end: growth is denied, the write faults, and the *reason* is the
    // ceiling rather than a generic trap — a safe failure is one a reader can act on.
    let wasm = wat::parse_str(
        r#"
        (module
          (memory (export "memory") 1)
          (func (export "run") (param i32 i32) (result i32)
            (drop (memory.grow (i32.const 512)))
            (i32.store (i32.const 16000000) (i32.const 1))
            (i32.const 0)))
        "#,
    )
    .expect("fixture compiles");

    let (status, body) = client
        .send(
            "POST",
            "/api/v1/operator/sandbox/run",
            Some(body_for(&wasm, 10_000_000)),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "a ceiling is a result, not a 500: {body}");
    assert_eq!(body["halt"], "memory", "{body}");
    assert_eq!(body["succeeded"], false, "{body}");
}

#[tokio::test]
async fn a_host_api_refuses_a_package_without_the_grant() {
    let mut h = Harness::new("perms").await;
    let mut client = h.operator("perms").await;

    let wasm = wat::parse_str(
        r#"
        (module
          (import "darkhold" "write" (func $write (param i32 i32) (result i32)))
          (memory (export "memory") 1)
          (func (export "run") (param i32 i32) (result i32)
            (drop (call $write (i32.const 0) (i32.const 8)))
            (i32.const 0)))
        "#,
    )
    .expect("fixture compiles");

    let mut body = body_for(&wasm, 10_000_000);
    // Granted `checkpoint` only: holding one API must not buy another.
    body["apis"] = json!(["checkpoint"]);
    let (status, response) = client
        .send("POST", "/api/v1/operator/sandbox/run", Some(body))
        .await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(response["halt"], "permission_denied", "{response}");
    assert_eq!(
        response["output_bytes"], 0,
        "the denied call wrote nothing, because it never ran: {response}"
    );
    assert_eq!(
        response["fuel_used"], 0,
        "and nothing ran at all — refused at link time: {response}"
    );
}

#[tokio::test]
async fn uninstalling_a_grant_removes_it_and_reinstalling_restores_it() {
    let h = Harness::new("grants").await;
    let db = h.tdb.db();
    let account = uuid::Uuid::new_v4().to_string();

    // A grant, as the product writes one.
    darkhold_db::marketplace::grant_extension(
        db,
        &account,
        "shelf-sorter",
        "1.0.0",
        &[darkhold_domain::extension::Capability::WorkRead],
    )
    .await
    .expect("grant");
    assert!(
        darkhold_db::marketplace::holds_grant(db, &account, "shelf-sorter", "work.read")
            .await
            .expect("read"),
        "the grant holds once given"
    );

    // Uninstall.
    darkhold_db::marketplace::revoke_extension(db, &account, "shelf-sorter")
        .await
        .expect("revoke");
    assert!(
        darkhold_db::marketplace::active_grants(db, &account)
            .await
            .expect("read")
            .is_empty(),
        "an uninstalled grant stops counting: revocation is a column, so the row remains as history"
    );
    assert!(
        !darkhold_db::marketplace::holds_grant(db, &account, "shelf-sorter", "work.read")
            .await
            .expect("read"),
        "and the executor's question is answered no"
    );
    assert_eq!(
        darkhold_db::marketplace::list_my_grants(db, &account)
            .await
            .expect("read")
            .len(),
        1,
        "the history is still there, because 'what did I install' and 'what may it do' are two questions"
    );

    // Reinstall. This is the one that used to fail: the primary key is (account, manifest_id), so a plain INSERT after a revocation was a
    // unique violation and an extension could never be re-granted.
    darkhold_db::marketplace::grant_extension(
        db,
        &account,
        "shelf-sorter",
        "1.1.0",
        &[darkhold_domain::extension::Capability::WorkRead],
    )
    .await
    .expect("re-grant after uninstall");
    assert!(
        darkhold_db::marketplace::holds_grant(db, &account, "shelf-sorter", "work.read")
            .await
            .expect("read"),
        "a second consent re-activates the row rather than colliding with it"
    );
    let active = darkhold_db::marketplace::active_grants(db, &account)
        .await
        .expect("read");
    assert_eq!(active.len(), 1);
    assert_eq!(
        active[0]["version"], "1.1.0",
        "the version recorded is the one just confirmed, not the one that was uninstalled"
    );
}

#[tokio::test]
async fn reset_is_reachable_after_a_broken_theme() {
    // A skin that no longer passes the validator — the state a reader reaches when the rules tighten under an old skin — must not be a
    // trap: `DELETE /me/skin` has to work without parsing the CSS, or the one control that returns a reader to the site's own interface is
    // behind the broken thing it exists to undo.
    let h = Harness::new("skin").await;
    let mut client = h.client();
    let account = client.register("skinny").await;

    // Written straight to the store, bypassing the route: the point is the state a *validating* write cannot produce.
    let broken = "body { color: red; } @import url('http://evil.test/x.css'); }}}";
    darkhold_db::reader_skin::put(h.tdb.db(), &account, broken)
        .await
        .expect("seed a skin the validator would refuse");

    let (status, body) = client.send("GET", "/api/v1/me/skin", None).await;
    assert_eq!(status, StatusCode::OK, "reading it is not an error either: {body}");
    assert_eq!(body["present"], true, "{body}");

    let (status, body) = client.send("DELETE", "/api/v1/me/skin", None).await;
    assert_eq!(status, StatusCode::OK, "reset is reachable: {body}");
    assert_eq!(body["present"], false, "{body}");

    let (status, body) = client.send("GET", "/api/v1/me/skin", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["present"], false,
        "and the reader is back on the site's own interface: {body}"
    );
}
