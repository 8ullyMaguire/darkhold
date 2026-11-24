//! Step 8: the configured abandonment window is published at `/api/v1/meta`.
//!
//! The UI must not hard-code four months. If `/meta` omits the number, every
//! card that explains an automatic abandonment would be guessing.

use axum::http::StatusCode;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;

#[tokio::test]
async fn meta_publishes_the_configured_abandonment_window() {
    let dir = test_support::scratch_dir("lifecycle-meta");
    let tdb = test_support::TestDb::connect_with_dir("lifecycle-meta", &dir).await;
    let mut config = Config::development_defaults();
    config.storage.root = dir;
    config.works.auto_abandon_after_months = 7;
    let mut client = test_support::TestClient::new(server::build_router(AppState::new(
        config,
        tdb.db().clone(),
    )));
    let (status, body) = client.get("/api/v1/meta").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["works"]["auto_abandon_after_months"], 7, "{body}");
    tdb.cleanup().await;
}
