//! M71-07 — the operator overview is a document, not another SPA shell.
//!
//! The requirement names an admin overview that renders live counts and an alert bar naming every red state with a
//! fix link, and refuses non-operators with 404. This test pins all three before any implementation exists: the route
//! is missing, so the first run fails with the SPA fallback or a 404.

use axum::body::Body;
use axum::http::{header, Request, StatusCode};

use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use test_support::{scratch_dir, sign_in_as, TestClient, TestDb};
use tower::ServiceExt;

const OPERATOR_EMAIL: &str = "overview-operator@example.com";
const OPERATOR_HANDLE: &str = "overviewoperator";
const OTHER_EMAIL: &str = "overview-other@example.com";
const OTHER_HANDLE: &str = "overviewother";

fn config_for(dir: &std::path::Path) -> Config {
    let mut config = Config::development_defaults();
    config.storage.root = dir.to_path_buf();
    config
}

struct Harness {
    _dir: std::path::PathBuf,
    tdb: TestDb,
    config: Config,
}

impl Harness {
    async fn new(tag: &str) -> Self {
        let dir = scratch_dir(tag);
        let tdb = TestDb::connect_with_dir(tag, &dir).await;
        let config = config_for(&dir);
        Self {
            _dir: dir,
            tdb,
            config,
        }
    }

    fn router(&self) -> axum::Router {
        server::build_router(AppState::new(self.config.clone(), self.tdb.db().clone()))
    }

    fn client(&self) -> TestClient {
        TestClient::new(self.router())
    }
}

/// HTML documents cannot be read through `TestClient::get`: that helper parses the body as JSON and gives back a
/// placeholder string. Read the raw body with the same cookie the API tests use.
async fn get_document(
    app: &axum::Router,
    uri: &str,
    session: Option<&str>,
) -> (StatusCode, String) {
    let mut builder = Request::builder().uri(uri);
    if let Some(token) = session {
        builder = builder.header(
            header::COOKIE,
            format!("{}={}", darkhold_app::auth::SESSION_COOKIE, token),
        );
    }
    let response = app
        .clone()
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

#[tokio::test]
async fn the_overview_names_live_red_states_and_hides_from_non_operators() {
    let mut h = Harness::new("admin_overview").await;

    // Register the operator first: config names an account that must already exist.
    let mut bootstrap = h.client();
    let operator_account =
        sign_in_as(&mut bootstrap, &h.tdb, OPERATOR_EMAIL, OPERATOR_HANDLE).await;
    drop(bootstrap);
    h.config.administration.operator_account_id =
        Some(operator_account.parse().expect("account uuid"));

    // A live red state from an existing store door, not a fixture rendered by the page.
    darkhold_db::imports::upsert_source(h.tdb.db(), "ao3", "AO3", "1", "[]")
        .await
        .expect("upsert source");
    darkhold_db::imports::set_source_health(h.tdb.db(), "ao3", "unavailable")
        .await
        .expect("set source health");

    let mut operator = h.client();
    sign_in_as(&mut operator, &h.tdb, OPERATOR_EMAIL, OPERATOR_HANDLE).await;
    let operator_cookie = operator
        .cookie(darkhold_app::auth::SESSION_COOKIE)
        .expect("session cookie")
        .to_owned();
    let router = h.router();
    let (status, text) = get_document(&router, "/admin", Some(&operator_cookie)).await;
    assert_eq!(status, StatusCode::OK, "operator gets the overview: {text}");
    println!(
        "OVERVIEW_BODY_SNIPPET={}",
        &text.replace('\n', " ")[..text.len().min(1200)]
    );
    assert!(
        !text.contains("<script"),
        "overview is a document before script"
    );
    assert!(text.contains("Operator overview"), "the page names itself");
    assert!(
        text.contains("Needs attention"),
        "red states get the alert bar"
    );
    assert!(
        text.contains("Source ao3 is unavailable"),
        "the red state is named in words"
    );
    assert!(
        text.contains("/admin/media-health"),
        "the red state has a fix link"
    );
    assert!(text.contains("Queue"), "live job counts are one group");
    assert!(text.contains("Sources"), "live source health is one group");
    assert!(text.contains("Media"), "live media counts are one group");

    let mut other = h.client();
    sign_in_as(&mut other, &h.tdb, OTHER_EMAIL, OTHER_HANDLE).await;
    let other_cookie = other
        .cookie(darkhold_app::auth::SESSION_COOKIE)
        .expect("session cookie")
        .to_owned();
    let (status, text) = get_document(&router, "/admin", Some(&other_cookie)).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a non-operator must not learn the overview exists: {text}"
    );

    let (status, text) = get_document(&router, "/admin", None).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "anonymous is refused by the session layer before the operator check: {text}"
    );
}
