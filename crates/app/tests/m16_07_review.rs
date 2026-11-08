//! M16-07 — §21.5's review workflow end to end.
//!
//! The rules live in `darkhold_domain::extension_review` and have their own unit tests; this file proves the *wiring*: that the routes
//! store what the state machine decided, that a refusal reaches the caller as a sentence rather than a 500, that publishing writes the
//! manifest state, and that revocation revokes the version's grants and names the rollback target.
//!
//! Three reviewers, three sessions, all operators: `require_operator` is the estate claim (`role:operator` on the session or the
//! configured operator account), so the test grants the role the way the issuer would — by writing it onto the session row.

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
        "darkhold-m1607-{tag}-{}-{:?}",
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

    /// A signed-in client whose session carries `role:operator`.
    async fn operator(&self, handle: &str) -> Client {
        let mut client = self.client();
        let account = client.register(handle).await;
        sqlx::query("UPDATE sessions SET roles = '[\"role:operator\"]' WHERE account_id = ?")
            .bind(&account)
            .execute(self.tdb.db().sqlite_pool().expect("sqlite"))
            .await
            .expect("grant the operator role to the session");
        // The role is read when the session is loaded, and it is loaded per request, so no rebuild is needed — but the client must have
        // the cookie the row belongs to, which registration already set.
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
                    "email": format!("m16-07-{handle}@example.test"),
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

fn message(body: &Value) -> String {
    body["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

#[tokio::test]
async fn a_package_walks_the_whole_workflow_and_lands_published() {
    let h = Harness::new("happy").await;
    // The author is an operator too, on purpose: the refusal that follows is §21.5's own rule ("the submitter cannot review their own
    // package"), not the operator door. An author who is not an operator is refused earlier and would prove nothing.
    let mut author = h.operator("author").await;

    let (status, body) = author
        .send(
            "POST",
            "/api/v1/extensions/shelf-sorter/submit",
            Some(json!({ "version": "1.0.0", "document": "{\"id\":\"shelf-sorter\"}" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["stage"], "submitted", "{body}");

    let mut reviewer_a = h.operator("reviewer-a").await;
    let mut reviewer_b = h.operator("reviewer-b").await;
    let mut reviewer_c = h.operator("reviewer-c").await;

    // Automated checks must run before any human stage, and the sandbox is where they come from.
    let (status, body) = reviewer_a
        .send(
            "POST",
            "/api/v1/extensions/reviews/shelf-sorter/1.0.0/advance",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        message(&body).contains("cannot move"),
        "the refusal is the rule's own sentence: {}",
        message(&body)
    );

    let (status, body) = reviewer_a
        .send(
            "POST",
            "/api/v1/extensions/reviews/shelf-sorter/1.0.0/checks",
            Some(json!({
                "checks": [
                    { "name": "sandbox.runs", "passed": true },
                    { "name": "imports.declared", "passed": true }
                ]
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["stage"], "automated_checks", "{body}");
    assert_eq!(body["checks_passed"], true, "{body}");

    // The author may not review their own package.
    let (status, body) = author
        .send(
            "POST",
            "/api/v1/extensions/reviews/shelf-sorter/1.0.0/advance",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(message(&body).contains("own package"), "{}", message(&body));

    let (status, body) = reviewer_a
        .send(
            "POST",
            "/api/v1/extensions/reviews/shelf-sorter/1.0.0/advance",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["stage"], "permission_review", "{body}");

    // The same operator cannot be the security reviewer too.
    let (status, body) = reviewer_a
        .send(
            "POST",
            "/api/v1/extensions/reviews/shelf-sorter/1.0.0/advance",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        message(&body).contains("two different people"),
        "{}",
        message(&body)
    );

    let (status, body) = reviewer_b
        .send(
            "POST",
            "/api/v1/extensions/reviews/shelf-sorter/1.0.0/advance",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["stage"], "security_review", "{body}");

    let (status, body) = reviewer_c
        .send(
            "POST",
            "/api/v1/extensions/reviews/shelf-sorter/1.0.0/advance",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["stage"], "quorum", "{body}");

    // One approval is not quorum, and it is not an error either: the vote counted and the package is simply not published.
    let (status, body) = reviewer_a
        .send(
            "POST",
            "/api/v1/extensions/reviews/shelf-sorter/1.0.0/approve",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["published"], false, "{body}");
    assert_eq!(body["quorum"]["have"], 1, "{body}");
    assert_eq!(body["quorum"]["need"], 2, "{body}");

    let (status, body) = reviewer_a
        .send(
            "POST",
            "/api/v1/extensions/reviews/shelf-sorter/1.0.0/approve",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        message(&body).contains("already approved"),
        "{}",
        message(&body)
    );

    let (status, body) = reviewer_b
        .send(
            "POST",
            "/api/v1/extensions/reviews/shelf-sorter/1.0.0/approve",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["published"], true, "{body}");
    assert_eq!(body["stage"], "published", "{body}");

    // Publishing is a manifest state, not only a review stage: a reader granting this package must find it approved.
    let extension = darkhold_db::marketplace::get_extension(h.tdb.db(), "shelf-sorter")
        .await
        .expect("read")
        .expect("the manifest exists");
    assert_eq!(extension["state"], "approved", "{extension}");

    // And the queue no longer holds it.
    let mut reviewer_b2 = h.operator("reviewer-b2").await;
    let (status, body) = reviewer_b2
        .send("GET", "/api/v1/extensions/reviews?stage=quorum", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["items"].as_array().map(Vec::len), Some(0), "{body}");
}

#[tokio::test]
async fn revocation_pulls_the_version_and_names_what_readers_fall_back_to() {
    let h = Harness::new("revoke").await;
    let mut author = h.client();
    let author_account = author.register("author").await;

    // Publish 1.0.0 through the workflow, then submit and publish 1.1.0 — one helper function would hide the point of the test, which is
    // that a second published version is what makes a rollback target exist.
    for version in ["1.0.0", "1.1.0"] {
        let (status, body) = author
            .send(
                "POST",
                "/api/v1/extensions/shelf-sorter/submit",
                Some(json!({ "version": version, "document": "{\"id\":\"shelf-sorter\"}" })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");

        // Handles are letters, digits, `_` and `-`: `1.0.0` has dots, so the version becomes a suffix without them.
        let tag = version.replace('.', "-");
        let mut a = h.operator(&format!("perm-{tag}")).await;
        let mut b = h.operator(&format!("sec-{tag}")).await;
        let mut c = h.operator(&format!("third-{tag}")).await;
        let path = format!("/api/v1/extensions/reviews/shelf-sorter/{version}");
        a.send(
            "POST",
            &format!("{path}/checks"),
            Some(json!({
                "checks": [{ "name": "sandbox.runs", "passed": true }]
            })),
        )
        .await;
        a.send("POST", &format!("{path}/advance"), None).await;
        b.send("POST", &format!("{path}/advance"), None).await;
        c.send("POST", &format!("{path}/advance"), None).await;
        a.send("POST", &format!("{path}/approve"), None).await;
        let (status, body) = b.send("POST", &format!("{path}/approve"), None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["published"], true, "{body}");
    }

    // A reader holds a grant on 1.1.0.
    darkhold_db::marketplace::grant_extension(
        h.tdb.db(),
        &author_account,
        "shelf-sorter",
        "1.1.0",
        &[darkhold_domain::extension::Capability::WorkRead],
    )
    .await
    .expect("grant");

    let mut d = h.operator("revoker").await;
    let (status, body) = d
        .send(
            "POST",
            "/api/v1/extensions/reviews/shelf-sorter/1.1.0/revoke",
            Some(json!({ "reason": "a sandbox escape was reported" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["stage"], "revoked", "{body}");
    assert_eq!(
        body["rollback_to"], "1.0.0",
        "the caller is told which version readers still have: {body}"
    );
    assert_eq!(
        body["grants_revoked"], 1,
        "the revoked version's grants stop counting: {body}"
    );
    assert!(
        !darkhold_db::marketplace::holds_grant(
            h.tdb.db(),
            &author_account,
            "shelf-sorter",
            "work.read"
        )
        .await
        .expect("read"),
        "and the reader's grant no longer holds for that version"
    );
    assert_eq!(
        body["reason"], "a sandbox escape was reported",
        "the reason is stored, because 'why was this pulled' is the question asked later: {body}"
    );
}

#[tokio::test]
async fn only_operators_see_the_queue() {
    let h = Harness::new("queue").await;
    let mut reader = h.client();
    reader.register("plain-reader").await;
    // One refusal shape for the whole operator surface (ADR 0030): `404`, because "yes, and you may not" tells a reader the
    // review queue exists.
    let (status, body) = reader.send("GET", "/api/v1/extensions/reviews", None).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "403 would confirm the review queue exists: {body}"
    );

    let mut operator = h.operator("queue-op").await;
    let (status, body) = operator
        .send("GET", "/api/v1/extensions/reviews?stage=nonsense", None)
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let (status, body) = operator
        .send("GET", "/api/v1/extensions/reviews", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["stage"], "permission_review", "{body}");
}

/// A manifest document the workflow can actually read, so the renewal clause has capabilities to compare.
fn manifest_json(permissions: &[&str]) -> String {
    json!({
        "id": "shelf-sorter",
        "version": 1,
        "category": "integration",
        "entrypoint": "main.wasm",
        "required_host_api_version": 1,
        "permissions": permissions,
        "resource_limits": { "memory_mib": 32, "fuel": 1_000_000, "host_calls_per_run": 64 },
        "supported_surfaces": ["library"],
        "license": "CC0-1.0",
        "network_allowlist": [],
        "pricing": "free",
    })
    .to_string()
}

#[tokio::test]
async fn a_version_that_asks_for_more_invalidates_the_older_grant() {
    let h = Harness::new("renewal").await;
    let mut author = h.operator("author").await;
    let reader_account = {
        let mut reader = h.client();
        reader.register("reader").await
    };

    // 1.0.0 asks for one capability and is published.
    let (status, body) = author
        .send(
            "POST",
            "/api/v1/extensions/shelf-sorter/submit",
            Some(json!({ "version": "1.0.0", "document": manifest_json(&["work.read"]) })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = author
        .send(
            "POST",
            "/api/v1/extensions/reviews/shelf-sorter/1.0.0/checks",
            Some(json!({ "checks": [{ "name": "sandbox.runs", "passed": true }] })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let mut perm = h.operator("perm").await;
    let mut sec = h.operator("sec").await;
    let mut third = h.operator("third").await;
    perm.send(
        "POST",
        "/api/v1/extensions/reviews/shelf-sorter/1.0.0/advance",
        None,
    )
    .await;
    sec.send(
        "POST",
        "/api/v1/extensions/reviews/shelf-sorter/1.0.0/advance",
        None,
    )
    .await;
    third
        .send(
            "POST",
            "/api/v1/extensions/reviews/shelf-sorter/1.0.0/advance",
            None,
        )
        .await;
    perm.send(
        "POST",
        "/api/v1/extensions/reviews/shelf-sorter/1.0.0/approve",
        None,
    )
    .await;
    let (status, body) = sec
        .send(
            "POST",
            "/api/v1/extensions/reviews/shelf-sorter/1.0.0/approve",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["published"], true, "{body}");

    // The reader consents to exactly what 1.0.0 asked for.
    darkhold_db::marketplace::grant_extension(
        h.tdb.db(),
        &reader_account,
        "shelf-sorter",
        "1.0.0",
        &[darkhold_domain::extension::Capability::WorkRead],
    )
    .await
    .expect("grant");
    assert!(darkhold_db::marketplace::holds_grant(
        h.tdb.db(),
        &reader_account,
        "shelf-sorter",
        "work.read"
    )
    .await
    .expect("read"));

    // 1.1.0 asks for one more.
    let (status, body) = author
        .send(
            "POST",
            "/api/v1/extensions/shelf-sorter/submit",
            Some(json!({
                "version": "1.1.0",
                "document": manifest_json(&["work.read", "network"])
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    perm.send(
        "POST",
        "/api/v1/extensions/reviews/shelf-sorter/1.1.0/checks",
        Some(json!({ "checks": [{ "name": "sandbox.runs", "passed": true }] })),
    )
    .await;
    perm.send(
        "POST",
        "/api/v1/extensions/reviews/shelf-sorter/1.1.0/advance",
        None,
    )
    .await;
    sec.send(
        "POST",
        "/api/v1/extensions/reviews/shelf-sorter/1.1.0/advance",
        None,
    )
    .await;
    third
        .send(
            "POST",
            "/api/v1/extensions/reviews/shelf-sorter/1.1.0/advance",
            None,
        )
        .await;
    perm.send(
        "POST",
        "/api/v1/extensions/reviews/shelf-sorter/1.1.0/approve",
        None,
    )
    .await;
    let (status, body) = sec
        .send(
            "POST",
            "/api/v1/extensions/reviews/shelf-sorter/1.1.0/approve",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["published"], true, "{body}");
    assert_eq!(
        body["renewed_consent"], 1,
        "the reader consented to work.read, not to network: {body}"
    );

    // And the grant genuinely stops counting, rather than only being reported as stopped.
    assert!(
        !darkhold_db::marketplace::holds_grant(
            h.tdb.db(),
            &reader_account,
            "shelf-sorter",
            "work.read"
        )
        .await
        .expect("read"),
        "approving a wider permission set must not extend the older consent"
    );
}
