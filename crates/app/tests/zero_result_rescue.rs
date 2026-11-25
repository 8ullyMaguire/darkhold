//! FS-06: a zero-result search offers named, actually-non-empty relaxations.

use darkhold_app::config::Config;
use darkhold_app::server::{self, set_trust_proxy};
use darkhold_app::state::AppState;
use darkhold_db::DatabaseConfig;
use serde_json::Value;
use std::path::PathBuf;
use tower::ServiceExt;

const A: &str = "0195f006-0000-4000-8000-000000000001";
const U: &str = "0195f006-0000-4000-8000-000000000002";
const FA: &str = "0195f006-0000-4000-8000-000000000011";
const FB: &str = "0195f006-0000-4000-8000-000000000012";
const TG: &str = "0195f006-0000-4000-8000-000000000013";
const TO: &str = "0195f006-0000-4000-8000-000000000014";
const W1: &str = "0195f006-0000-4000-8000-000000000021";
const W2: &str = "0195f006-0000-4000-8000-000000000022";
const W3: &str = "0195f006-0000-4000-8000-000000000023";

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("darkhold-fs06-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn config(dir: &PathBuf) -> Config {
    let mut config = Config::development_defaults();
    config.storage.root = dir.clone();
    config.database = DatabaseConfig::new(format!(
        "sqlite://{}/darkhold.sqlite?mode=rwc",
        dir.display()
    ));
    config
}

async fn exec(db: &darkhold_db::Database, sql: &str) {
    sqlx::query(sql)
        .execute(db.sqlite_pool().expect("sqlite"))
        .await
        .unwrap_or_else(|e| panic!("fixture: {sql}: {e}"));
}

async fn get(app: &axum::Router, uri: &str) -> Value {
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri(uri)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK, "GET {uri}");
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).expect("JSON body")
}

#[tokio::test]
async fn zero_result_query_offers_three_named_non_empty_relaxations() {
    set_trust_proxy(false);
    let dir = scratch();
    let tdb = test_support::TestDb::connect_with_dir("fs06", &dir).await;
    let db = tdb.db();
    exec(db, &format!("INSERT INTO accounts (id,status,email,age_state,created_at,updated_at,version) VALUES ('{A}','active','fs06@test.dev','adult','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)")).await;
    exec(db, &format!("INSERT INTO pseuds (id,account_id,handle,display_name,discoverability,created_at,updated_at,version) VALUES ('{U}','{A}','fs06','FS06','listed','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)")).await;
    for (node, kind, name) in [
        (FA, "fandom", "alpha"),
        (FB, "fandom", "beta"),
        (TG, "tag", "good"),
        (TO, "tag", "other"),
    ] {
        exec(db, &format!("INSERT INTO canonical_entities (id,kind,canonical,norm,created_at) VALUES ('{node}','{kind}','{name}','{name}','2026-01-01T00:00:00Z')")).await;
    }
    for (work, title) in [(W1, "One"), (W2, "Two"), (W3, "Three")] {
        exec(db, &format!("INSERT INTO works (id,owner_pseud_id,title,visibility,lifecycle,completion,created_at,updated_at) VALUES ('{work}','{U}','{title}','public','published','complete','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')")).await;
    }
    for (work, node) in [(W1, FA), (W1, TG), (W2, FB), (W2, TG), (W3, FA), (W3, TO)] {
        exec(db, &format!("INSERT INTO work_tags (work_id,node_id,prominence,added_at) VALUES ('{work}','{node}',1,'2026-01-01T00:00:00Z')")).await;
    }

    let app = server::build_router(AppState::new(config(&dir), db.clone()));
    let zero = get(
        &app,
        "/api/v1/search?q=tag%3Aimpossible%20AND%20fandom%3Aalpha%20AND%20tag%3Agood",
    )
    .await;
    assert_eq!(zero["items"].as_array().map(Vec::len), Some(0));
    let relaxations = zero["relaxations"].as_array().expect("relaxations array");
    assert_eq!(relaxations.len(), 3, "expected three named removals: {zero}");
    let labels = relaxations
        .iter()
        .map(|r| r["label"].as_str().unwrap_or(""))
        .collect::<Vec<_>>();
    assert!(labels.contains(&"Remove tag:impossible"));
    assert!(labels.contains(&"Remove tag:impossible and fandom:alpha"));
    assert!(labels.contains(&"Remove tag:impossible and tag:good"));

    for relaxation in relaxations {
        let candidate = relaxation["query"].as_str().expect("candidate query");
        assert!(!candidate.contains("tag:impossible"));
        let followed = get(
            &app,
            &format!(
                "/api/v1/search?q={}",
                candidate.replace(':', "%3A").replace(' ', "+")
            ),
        )
        .await;
        assert!(
            followed["items"].as_array().map(Vec::len).unwrap_or(0) > 0,
            "relaxation {candidate} must return rows"
        );
    }
}
