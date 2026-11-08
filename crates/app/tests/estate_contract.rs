//! The estate's meta and health contract, asserted on the wire (M72-05).
//!
//! Four products, four answers to "what are you?" and "are you alive?":
//!
//! * this one answers a full document at `/api/v1/meta`, and `/healthz` used to fall through to the SPA
//!   fallback — `200 text/html`, a page, to a monitor that asked whether the process is alive;
//! * ascend answers `{status: "ok"}` at the root `/healthz` and 404s at `/api/v1/meta` (it serves `/api/meta`);
//! * kindred has no meta document at all and carries its algorithm budget inside `/api/v1/health`;
//! * ficnexus mounts `/api`.
//!
//! The contract is the smallest thing that makes an uptime board and a `dh` client one expression:
//!
//! ```json
//! // GET /api/v1/meta
//! { "name": "…", "version": "…", "build": "…", "api_version": "v1", "environment": "…", "base_url": "…" }
//! // GET /healthz
//! { "status": "ok", "version": "…" }
//! ```
//!
//! Product-specific numbers (this product's `policy` and `decisions` blocks) live *beside* the common fields,
//! never instead of them — which is why the assertions below check for the presence of the six named fields and
//! do not care what else is in the document.
//!
//! The liveness answer must not touch the database: a health check that fails because the database is down gets
//! a perfectly healthy process killed by a supervisor that reads liveness as "restart me". That distinction is
//! older than this contract (`health/live` vs `health/ready`) and `/healthz` is an alias for the first one, not a
//! third opinion.

use std::path::PathBuf;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::Database;
use serde_json::Value;
use test_support::TestDb;
use tower::ServiceExt;

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-estate-contract-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

async fn get(app: Router, uri: &str) -> (StatusCode, Option<String>, Vec<u8>) {
    let request = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(Body::empty())
        .expect("request");
    let response = app.oneshot(request).await.expect("response");
    let status = response.status();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let bytes = axum::body::to_bytes(response.into_body(), 256 * 1024)
        .await
        .expect("body")
        .to_vec();
    (status, content_type, bytes)
}

async fn router() -> Router {
    let dir = scratch_dir("wire");
    let tdb = TestDb::connect_with_dir("estate_contract", &dir).await;
    let db: Database = tdb.db().clone();
    let mut config = Config::development_defaults();
    config.storage.root = dir;
    server::build_router(AppState::new(config, db))
}

/// `/healthz` answers JSON, and the JSON names the version. A monitor reads the status code; a person asking
/// "which build is on that box?" reads the body, and both questions arrive at this address across the estate.
#[tokio::test]
async fn healthz_answers_json_naming_the_version() {
    let app = router().await;
    let (status, content_type, bytes) = get(app, "/healthz").await;

    assert_eq!(status, StatusCode::OK);
    let content_type = content_type.unwrap_or_default();
    assert!(
        content_type.starts_with("application/json"),
        "`/healthz` answered {content_type:?} — a page to a monitor that asked whether the process is alive"
    );

    let body: Value = serde_json::from_slice(&bytes).expect("healthz must be JSON");
    assert_eq!(body["status"], "ok", "{body}");
    let version = body["version"].as_str().unwrap_or_default();
    assert!(!version.is_empty(), "no `version` in {body}");
}

/// `/api/v1/meta` names the product, its version and build, the API version it speaks, where it runs and
/// where it lives. Six fields, because that is what a client needs to say "I am talking to darkhold v0.1.0,
/// api v1, in production, at this URL" without a second request.
#[tokio::test]
async fn meta_names_the_product_version_and_api() {
    let app = router().await;
    let (status, content_type, bytes) = get(app, "/api/v1/meta").await;

    assert_eq!(status, StatusCode::OK);
    assert!(content_type
        .unwrap_or_default()
        .starts_with("application/json"));

    let body: Value = serde_json::from_slice(&bytes).expect("meta must be JSON");
    for field in [
        "name",
        "version",
        "build",
        "api_version",
        "environment",
        "base_url",
    ] {
        let value = body
            .get(field)
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("`{field}` missing from the meta document: {body}"));
        assert!(!value.is_empty(), "`{field}` is empty in {body}");
    }

    // The API version is a version, not a path: a client composes the root from it and its own convention.
    assert_eq!(body["api_version"], "v1", "{body}");
    // The base URL is absolute — a relative one cannot be pasted into a federation document.
    assert!(
        body["base_url"]
            .as_str()
            .is_some_and(|url| url.starts_with("http")),
        "`base_url` is not absolute: {body}"
    );
}

/// The two names answer the same document: `/healthz` is an alias for `health/live`, not a third opinion that
/// can drift away from it.
#[tokio::test]
async fn healthz_and_health_live_agree() {
    let app = router().await;
    let (_, _, healthz) = get(app.clone(), "/healthz").await;
    let (_, _, live) = get(app, "/health/live").await;

    let healthz: Value = serde_json::from_slice(&healthz).expect("healthz json");
    let live: Value = serde_json::from_slice(&live).expect("live json");
    assert_eq!(healthz["status"], live["status"]);
    assert_eq!(healthz["version"], live["version"]);
    assert_eq!(healthz["build"], live["build"]);
}
