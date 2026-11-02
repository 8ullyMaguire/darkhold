//! The description document is **served**, not merely built (M69-25).
//!
//! The unit tests in `routes::opensearch` prove the XML is right. They cannot
//! prove anything reads it: a `describe()` nothing mounts is a feature that
//! exists in the source and nowhere else. That is the failure this file is for,
//! and it is the one that survives review, because the function is tested,
//! documented and correct.
//!
//! Two claims, and the second is the one worth having:
//!
//! * `/opensearch.xml` answers 200 with its own content type — a browser that
//!   receives `text/html` refuses the document, and the reader sees nothing.
//! * the shell links it, so the feature is reachable rather than discoverable
//!   by anyone who guesses the URL.

use axum::http::StatusCode;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;

fn router_for(tdb: &test_support::TestDb, dir: &std::path::Path) -> axum::Router {
    let mut config = Config::development_defaults();
    config.storage.root = dir.to_path_buf();
    // The rate limiter's bucket map is process-global; clearing only ever adds
    // budget, and this file makes no request that wants a limit to trip.
    darkhold_app::limiter::clear_buckets();
    server::build_router(AppState::new(config, tdb.db().clone()))
}

#[tokio::test]
async fn the_description_document_is_served_under_its_own_content_type() {
    let dir = test_support::scratch_dir("opensearch_served");
    let tdb = test_support::TestDb::connect_with_dir("opensearch-served", &dir).await;

    let mut client = test_support::TestClient::new(router_for(&tdb, &dir));
    let (status, headers, body) = client.get_with_headers("/opensearch.xml").await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    assert!(
        content_type.starts_with("application/opensearchdescription+xml"),
        "a browser refuses the document without this type, got {content_type:?}"
    );
    // The body is not JSON, so the harness hands it back as text — the first
    // bytes are enough to name what was served.
    // The harness snapshots the first 200 bytes of a non-JSON body, so this
    // asserts what is in that window and leaves the template to the unit test
    // that can read the whole document.
    let text = body.as_str().unwrap_or_default();
    assert!(text.contains("OpenSearchDescription"), "{text}");
    assert!(text.contains("<ShortName>"), "{text}");

    tdb.cleanup().await;
}

#[tokio::test]
async fn the_page_shell_offers_the_engine_to_the_browser() {
    // The link lives in the served shell, not in a component: OpenSearch is read
    // by the browser before any script runs.
    let shell = include_str!("../../../frontend/index.html");
    assert!(
        shell.contains(r#"rel="search""#),
        "the shell must link the description document"
    );
    assert!(
        shell.contains(r#"href="/opensearch.xml""#),
        "the link must point at the route this file proves is served"
    );
    assert!(
        shell.contains("application/opensearchdescription+xml"),
        "the link needs the type, or the browser ignores it"
    );
}
