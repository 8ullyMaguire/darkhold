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
//! | forks keep attribution and request their own grants | `a_fork_keeps_attribution_and_starts_without_grants` |
//! | custom engines honour opt-out | `an_opted_out_account_is_not_handed_to_an_external_engine` |
//! | recipes honour opt-out | `a_recipe_is_applied_and_it_asks_nothing_of_an_opted_out_reader` |
//! | a recipe is a closed composition | `a_recipe_that_carries_anything_else_is_refused_by_name` |
//!
//! `revoked packages stop running` is the sandbox crate's `revocation_stops_a_running_module_at_its_next_host_call` — the flag is polled
//! by the host imports; the review workflow's revoke door calls the same function, so the claim has one implementation and two callers.
//!
//! The recipe half of the last clause is the interesting one, because the answer is *not* "refuse the opted-out reader": a declarative
//! recipe composes strategies the instance already runs, so there is nothing to hand outside and `custom_processing: refuse` does not
//! apply to it. See `crates/domain/src/recipe_document.rs` for why that is a property of the schema (the document has no field that could
//! carry an address) rather than a promise.

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

/// The first text column of a row, for the one-line lookups this file does.
pub fn row_id(row: sqlx::sqlite::SqliteRow) -> String {
    use sqlx::Row as _;
    row.get::<String, _>(0)
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
    assert_eq!(
        status,
        StatusCode::OK,
        "a ceiling is a result, not a 500: {body}"
    );
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
    assert_eq!(
        status,
        StatusCode::OK,
        "reading it is not an error either: {body}"
    );
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

#[tokio::test]
async fn a_fork_keeps_attribution_and_starts_without_grants() {
    // §21's claim has two halves and the second is the one that matters: a fork keeps the original's name on it, and it does **not**
    // inherit the original's permissions. A reader consented to `shelf-sorter`; `shelf-sorter-tweaked` is a different package.
    let h = Harness::new("fork").await;
    let mut client = h.client();
    let original_author = client.register("original").await;

    // The manifest row is written straight to the store: there is no submission route yet (M16-07 owns it), and a fork test that needed
    // the review workflow would be testing the workflow instead of the fork.
    let document =
        json!({ "id": "shelf-sorter", "name": "Shelf sorter", "author": "orig" }).to_string();
    sqlx::query(
        "INSERT INTO extension_manifests (id, version, document, submitted_by, state, created_at)
         VALUES ('shelf-sorter', '1.0.0', ?, ?, 'approved', '2026-11-01T00:00:00Z')",
    )
    .bind(&document)
    .bind(&original_author)
    .execute(h.tdb.db().sqlite_pool().expect("sqlite"))
    .await
    .expect("seed a published manifest");

    // The original's author holds a grant on the original. The fork must not inherit it.
    darkhold_db::marketplace::grant_extension(
        h.tdb.db(),
        &original_author,
        "shelf-sorter",
        "1.0.0",
        &[darkhold_domain::extension::Capability::WorkRead],
    )
    .await
    .expect("grant");

    let (status, body) = client
        .send(
            "POST",
            "/api/v1/extensions/shelf-sorter/fork",
            Some(json!({ "id": "shelf-sorter-tweaked", "version": "0.1.0" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["derived_from"], "shelf-sorter",
        "attribution is kept: {body}"
    );
    assert_eq!(body["derived_from_version"], "1.0.0", "{body}");
    assert_eq!(
        body["state"], "pending",
        "an approved package's copy is not approved; approval is a judgement about a package: {body}"
    );
    assert_eq!(
        body["grants"],
        json!([]),
        "and nothing was granted, because consent named the original: {body}"
    );

    assert!(
        !darkhold_db::marketplace::holds_grant(
            h.tdb.db(),
            &original_author,
            "shelf-sorter-tweaked",
            "work.read"
        )
        .await
        .expect("read"),
        "the forker's own grant does not follow them onto the fork"
    );
    assert!(
        darkhold_db::marketplace::active_grants(h.tdb.db(), &original_author)
            .await
            .expect("read")
            .iter()
            .all(|grant| grant["manifest_id"] == "shelf-sorter"),
        "the original grant is untouched"
    );

    // The document is byte-identical: a fork that rewrote the manifest would no longer be the thing its author signed.
    let forked = darkhold_db::marketplace::get_extension(h.tdb.db(), "shelf-sorter-tweaked")
        .await
        .expect("read")
        .expect("the fork exists");
    assert_eq!(
        forked["derived_from"], "shelf-sorter",
        "read back from the store, not from the response the fork built: the first version of this assertion checked the handler's own JSON and stayed green when the column was written as NULL"
    );
    assert_eq!(forked["document"], document, "{forked}");

    // And a fork cannot take the original's name, or the marketplace would have two packages called `shelf-sorter` and one review.
    let (status, body) = client
        .send(
            "POST",
            "/api/v1/extensions/shelf-sorter/fork",
            Some(json!({ "id": "shelf-sorter", "version": "2.0.0" })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    // A slug nobody published is a 404, not an empty fork.
    let (status, body) = client
        .send(
            "POST",
            "/api/v1/extensions/no-such-package/fork",
            Some(json!({ "id": "whatever", "version": "0.1.0" })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

#[tokio::test]
async fn an_opted_out_account_is_not_handed_to_an_external_engine() {
    // §21's last clause. The instance's narration engine is an external program (Piper), so asking for a narration is
    // asking for the reader's text to be handed to something outside the process. `custom_processing: refuse` stops it —
    // and the *message* is what makes the opt-out verifiable: the refusal names the setting rather than blaming the host.
    let mut h = Harness::new("optout").await;
    // The narration surface is an extension: an instance that switches it off answers `404 MODULE_UNAVAILABLE`, and a test about an
    // opt-out inside the surface has to have the surface on. Turning it on here is also a check that the switch and the guarantee are
    // independent — the door refusing for two different reasons must not be one reason wearing two messages.
    h.config.modules.enabled.push("narration".to_owned());
    let mut client = h.client();
    let user = client.register("optedout").await;

    let pseud: String = sqlx::query("SELECT id FROM pseuds WHERE account_id = ?")
        .bind(&user)
        .fetch_one(h.tdb.db().sqlite_pool().expect("sqlite"))
        .await
        .map(crate::row_id)
        .expect("the account has a pseud");
    let pseud: darkhold_domain::ids::PseudId = pseud.parse().expect("pseud id parses");
    let work = darkhold_db::content::create_work(h.tdb.db(), pseud, "Opted out work", None)
        .await
        .expect("create a work the session contributes to");
    let work = work.id.to_string();

    // Refused, and refused *before* the engine: on this host there is no narration engine at all, so a message about
    // Piper would prove nothing about the opt-out.
    let (status, body) = client
        .send(
            "POST",
            &format!("/api/v1/works/{work}/editions"),
            Some(json!({ "provider": "piper" })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let message = body["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(
        message.contains("cannot narrate"),
        "with the default (`allow`) the request reaches the engine check and fails on the host, not on consent: {message}"
    );

    // Throw the switch through the product's own door, so this also proves the key is settable.
    let (status, body) = client
        .send(
            "PATCH",
            "/api/v1/settings/privacy",
            Some(json!({ "changes": { "custom_processing": "refuse" } })),
        )
        .await;
    assert!(
        status == StatusCode::OK,
        "the key is settable through the same door as every other one, which is what makes it a setting rather than a constant: {body}"
    );

    let (status, body) = client
        .send(
            "POST",
            &format!("/api/v1/works/{work}/editions"),
            Some(json!({ "provider": "piper" })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let message = body["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(
        message.contains("custom_processing"),
        "the refusal names the setting the reader threw, not the host's engine: {message}"
    );
}

/// Publish an approved package with a real manifest, so the dispatch route's strict parse has something valid
/// to read. Category and permissions are parameters because the point of these tests is which category does
/// what.
async fn publish_approved(h: &Harness, id: &str, category: &str, permissions: &[&str]) {
    let document = json!({
        "id": id,
        "version": 1,
        "category": category,
        "entrypoint": "main.wasm",
        "required_host_api_version": 1,
        "permissions": permissions,
        "resource_limits": { "memory_mib": 32, "fuel": 1_000_000, "host_calls_per_run": 64 },
        "supported_surfaces": ["library"],
        "license": "MIT",
        "network_allowlist": [],
        "pricing": "free",
    })
    .to_string();
    sqlx::query(
        "INSERT INTO extension_manifests (id, version, document, submitted_by, state, created_at)
         VALUES (?, '1.0.0', ?, 'host', 'approved', '2026-11-01T00:00:00Z')",
    )
    .bind(id)
    .bind(&document)
    .execute(h.tdb.db().sqlite_pool().expect("sqlite"))
    .await
    .expect("seed an approved package");
}

#[tokio::test]
async fn a_recipe_is_applied_and_it_asks_nothing_of_an_opted_out_reader() {
    // The recipe half of §21's last clause, and the answer is the opposite of the engine's: a declarative
    // recipe composes strategies the instance already runs, so it hands nothing outside and consent does not
    // govern it. Refusing here would be a false positive that teaches readers the setting is noise.
    let h = Harness::new("recipe-apply").await;
    publish_approved(&h, "mood-blend", "declarative_recipe", &[]).await;

    let mut client = h.client();
    client.register("recipe-reader").await;

    let (status, body) = client
        .send(
            "POST",
            "/api/v1/extensions/mood-blend/install",
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "install: {body}");

    // Opt out of custom processing — the same switch the engine test throws.
    let (status, body) = client
        .send(
            "PATCH",
            "/api/v1/settings/privacy",
            Some(json!({ "changes": { "custom_processing": "refuse" } })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let strategies = darkhold_app::rec_engine::available_strategies();
    assert!(
        !strategies.is_empty(),
        "a recipe needs at least one strategy to compose"
    );
    let first = strategies[0].clone();
    let (status, body) = client
        .send(
            "POST",
            "/api/v1/extensions/mood-blend/apply-recipe",
            Some(json!({
                "recipe": { "stages": [{ "strategy": first, "weight": 1.0 }] },
                "name": "my blend",
            })),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a declarative recipe processes nothing of the reader's outside the host, so an opt-out must not block it: {body}"
    );
    assert_eq!(body["custom_processing_required"], json!(false), "{body}");
    assert_eq!(body["stages"][0]["strategy"], json!(first), "{body}");

    // It is a real recipe, owned by the reader who asked.
    let (status, body) = client.send("GET", "/api/v1/recipes/list", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let recipes = body["recipes"]
        .as_array()
        .or_else(|| body.as_array())
        .expect("a recipe list");
    assert!(
        recipes.iter().any(|r| r["name"] == json!("my blend")),
        "the applied recipe is the reader's own row: {body}"
    );

    // And the same reader is refused an *engine* package, because an engine is what processes.
    publish_approved(&h, "oracle-engine", "recommendation_engine", &[]).await;
    client
        .send(
            "POST",
            "/api/v1/extensions/oracle-engine/install",
            Some(json!({})),
        )
        .await;
    let (status, body) = client
        .send(
            "POST",
            "/api/v1/extensions/oracle-engine/apply-recipe",
            Some(json!({ "recipe": { "stages": [{ "strategy": first, "weight": 0.5 }] } })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let message = body["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(
        message.contains("custom_processing"),
        "the opt-out names the setting rather than blaming the package: {message}"
    );
}

#[tokio::test]
async fn a_recipe_that_carries_anything_else_is_refused_by_name() {
    // The schema is the mechanism: a document that may only carry strategy names cannot carry an address,
    // which is *why* the opt-out question does not arise for it.
    let h = Harness::new("recipe-closed").await;
    publish_approved(&h, "mood-blend", "declarative_recipe", &[]).await;

    let mut client = h.client();
    client.register("recipe-reader").await;
    client
        .send(
            "POST",
            "/api/v1/extensions/mood-blend/install",
            Some(json!({})),
        )
        .await;

    let strategies = darkhold_app::rec_engine::available_strategies();
    let first = strategies[0].clone();

    // A top-level key that is not `stages` — where a callback URL would hide.
    let (status, body) = client
        .send(
            "POST",
            "/api/v1/extensions/mood-blend/apply-recipe",
            Some(json!({
                "recipe": {
                    "stages": [{ "strategy": first, "weight": 1.0 }],
                    "endpoint": "https://collect.example/v1",
                }
            })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("endpoint"),
        "the refusal names the offending key: {body}"
    );

    // A strategy the instance does not run is refused with the menu of ones it does.
    let (status, body) = client
        .send(
            "POST",
            "/api/v1/extensions/mood-blend/apply-recipe",
            Some(json!({ "recipe": { "stages": [{ "strategy": "oracle", "weight": 1.0 }] } })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let message = body["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(
        message.contains(&first),
        "the refusal offers the strategies it does run: {message}"
    );

    // Nothing was stored by either refusal.
    let (_, body) = client.send("GET", "/api/v1/recipes/list", None).await;
    let recipes = body["recipes"]
        .as_array()
        .or_else(|| body.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        recipes.is_empty(),
        "a refused recipe is not a stored recipe: {body}"
    );

    // A package that is not a recipe cannot be applied through this door at all.
    publish_approved(&h, "widget", "reader_widget", &[]).await;
    client
        .send("POST", "/api/v1/extensions/widget/install", Some(json!({})))
        .await;
    let (status, body) = client
        .send(
            "POST",
            "/api/v1/extensions/widget/apply-recipe",
            Some(json!({ "recipe": { "stages": [{ "strategy": first, "weight": 1.0 }] } })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("reader_widget"),
        "{body}"
    );
}
