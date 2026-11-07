//! M64-08 through the real router: the declared map, its version, and what it reaches.
//!
//! The map is configuration, so this test's job is the wiring a unit test cannot see: the config is read at request time, the version
//! travels with the answer, an unmapped fandom is answered rather than refused, and the works come from the declared neighbours only —
//! with an instance that has published no map serving the same page with no edges instead of failing.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::{Backend, Database};
use serde_json::Value;
use test_support::TestDb;
use tower::ServiceExt;

const T0: &str = "2026-01-01T00:00:00Z";
const ACCOUNT: &str = "01955c00-0000-4000-8000-00000000a000";
const PSEUD: &str = "01955c00-0000-4000-8000-00000000a001";
const WORM: &str = "01955c00-0000-4000-8000-00000000a010";
const WARD: &str = "01955c00-0000-4000-8000-00000000a011";
const WARD_WORK: &str = "01955c00-0000-4000-8000-00000000a021";

struct Harness {
    _tdb: TestDb,
    _dir: std::path::PathBuf,
    config: Config,
    db: Database,
}

impl Harness {
    async fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "darkhold-m64-08-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        let mut config = Config::development_defaults();
        config.storage.root = dir.clone();
        let tdb = TestDb::connect_with_dir(tag, &dir).await;
        let db = tdb.db().clone();
        Self {
            _tdb: tdb,
            _dir: dir,
            config,
            db,
        }
    }

    fn app(&self) -> axum::Router {
        server::build_router(AppState::new(self.config.clone(), self.db.clone()))
    }
}

async fn exec(db: &Database, statement: &str) {
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(statement)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await
                .unwrap_or_else(|error| panic!("{statement}\n{error}"));
        }
        Backend::Postgres => {
            sqlx::query(statement)
                .execute(db.postgres_pool().expect("postgres"))
                .await
                .unwrap_or_else(|error| panic!("{statement}\n{error}"));
        }
    }
}

async fn seed(db: &Database) {
    exec(
        db,
        &format!(
            "INSERT INTO accounts (id, email, status, age_state, created_at, updated_at, permission_statement)
             VALUES ('{ACCOUNT}','fg@example.invalid','active','adult','{T0}','{T0}','minimal')"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at)
             VALUES ('{PSEUD}','{ACCOUNT}','fgw','fgw','{T0}','{T0}')"
        ),
    )
    .await;
    for (id, name) in [(WORM, "Worm"), (WARD, "Ward")] {
        exec(
            db,
            &format!(
                "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at, review_status, signal_count)
                 VALUES ('{id}','fandom','{name}','{}','{T0}','confirmed',0)",
                name.to_lowercase()
            ),
        )
        .await;
    }
    exec(
        db,
        &format!(
            "INSERT INTO works (id, owner_pseud_id, title, summary, lifecycle, visibility, completion,
                                created_at, updated_at, generated_content_posture)
             VALUES ('{WARD_WORK}','{PSEUD}','Ward story','','published','public','complete','{T0}','{T0}','forbid')"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO work_tags (work_id, node_id, prominence, added_at)
             VALUES ('{WARD_WORK}','{WARD}',1,'{T0}')"
        ),
    )
    .await;
}

async fn get(app: axum::Router, path: &str) -> (StatusCode, Value) {
    let response = app
        .oneshot(
            Request::builder()
                .uri(path)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("body");
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

/// The `Worm <-> Ward` case, from config to JSON.
#[tokio::test]
async fn the_declared_map_is_served_with_its_version_and_the_works_it_reaches() {
    let harness = Harness::new("declared").await;
    seed(&harness.db).await;

    // Nothing configured yet: every instance's state until an operator publishes a map.
    let (status, body) = get(harness.app(), "/api/v1/fandoms/Worm/related").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(body["graph_version"], "unpublished");
    assert_eq!(body["neighbours"].as_array().map(Vec::len), Some(0));
    assert_eq!(body["works"].as_array().map(Vec::len), Some(0));

    let mut configured = harness.config.clone();
    configured.fandom_graph = darkhold_domain::fandom_graph::FandomGraph {
        version: "2026-11-07".to_owned(),
        edges: vec![darkhold_domain::fandom_graph::Edge {
            from: "Worm".to_owned(),
            to: "Ward".to_owned(),
            weight: 0.9,
        }],
    };
    configured.validate().expect("a valid map");
    let app = server::build_router(AppState::new(configured, harness.db.clone()));

    let (status, body) = get(app, "/api/v1/fandoms/Worm/related").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(body["graph_version"], "2026-11-07", "the page can name its map");
    assert_eq!(body["fandom"], "Worm");
    assert_eq!(body["neighbours"][0]["name"], "Ward");
    assert_eq!(body["neighbours"][0]["weight"], 0.9);
    assert_eq!(
        body["works"][0]["title"], "Ward story",
        "the Ward work is reachable from Worm: {body}"
    );
    assert_eq!(body["works"][0]["weight"], 0.9);

    // The other end of the same edge answers symmetrically.
    let mut configured = harness.config.clone();
    configured.fandom_graph = darkhold_domain::fandom_graph::FandomGraph {
        version: "2026-11-07".to_owned(),
        edges: vec![darkhold_domain::fandom_graph::Edge {
            from: "Worm".to_owned(),
            to: "Ward".to_owned(),
            weight: 0.9,
        }],
    };
    let app = server::build_router(AppState::new(configured, harness.db.clone()));
    let (status, body) = get(app, "/api/v1/fandoms/Ward/related").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(body["neighbours"][0]["name"], "Worm");

    // A fandom the map does not mention is answered, not refused: nothing is declared next to it.
    let mut configured = harness.config.clone();
    configured.fandom_graph = darkhold_domain::fandom_graph::FandomGraph {
        version: "2026-11-07".to_owned(),
        edges: vec![darkhold_domain::fandom_graph::Edge {
            from: "Worm".to_owned(),
            to: "Ward".to_owned(),
            weight: 0.9,
        }],
    };
    let app = server::build_router(AppState::new(configured, harness.db.clone()));
    let (status, body) = get(app, "/api/v1/fandoms/Bleach/related").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(body["neighbours"].as_array().map(Vec::len), Some(0));
    assert_eq!(body["works"].as_array().map(Vec::len), Some(0));
    assert_eq!(body["graph_version"], "2026-11-07");
}
