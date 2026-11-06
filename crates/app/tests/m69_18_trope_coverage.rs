//! M69-18 — the coverage route is silent until an operator publishes it.
//!
//! The db module proves the checklist's arithmetic. This proves the **door**: with `taxonomy.trope_coverage` off the route answers
//! `404` (the shape of a feature that was never installed, not a `403` announcing a door a reader may not open), and with it on the
//! same instance serves the checklist. One instance, two answers, one switch — that is the whole decision this card asked for.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server::{self, set_trust_proxy};
use darkhold_app::state::AppState;
use darkhold_db::DatabaseConfig;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tower::ServiceExt;

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-troperoute-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn config_for(dir: &Path, published: bool) -> Config {
    let mut config = Config::development_defaults();
    config.storage.root = dir.to_path_buf();
    config.database = DatabaseConfig::new(format!(
        "sqlite://{}/darkhold.sqlite?mode=rwc",
        dir.display()
    ));
    config.taxonomy.trope_coverage = published;
    config
}

const ACCOUNT: &str = "01955900-0000-4000-8000-000000000001";
const PSEUD: &str = "01955900-0000-4000-8000-000000000002";
const WORK: &str = "01955900-0000-4000-8000-000000000003";
const COMFORT: &str = "01955900-0000-4000-8000-000000000004";

struct Harness {
    _dir: PathBuf,
    tdb: test_support::TestDb,
    dir: PathBuf,
}

impl Harness {
    async fn new(tag: &str) -> Self {
        set_trust_proxy(false);
        let dir = scratch_dir(tag);
        let tdb = test_support::TestDb::connect_with_dir(tag, &dir).await;
        Self {
            _dir: dir.clone(),
            tdb,
            dir,
        }
    }

    fn client(&self, published: bool) -> axum::Router {
        server::build_router(AppState::new(
            config_for(&self.dir, published),
            self.tdb.db().clone(),
        ))
    }

    async fn exec(&self, statement: &str) {
        sqlx::query(statement)
            .execute(self.tdb.db().sqlite_pool().expect("sqlite"))
            .await
            .unwrap_or_else(|error| panic!("{statement}\n{error}"));
    }

    async fn seed(&self) {
        self.exec(&format!(
            "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version)
             VALUES ('{ACCOUNT}','active','trope@example.invalid','adult',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ))
        .await;
        self.exec(&format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at,
                                 updated_at, version)
             VALUES ('{PSEUD}','{ACCOUNT}','tropewriter','Trope Writer','listed',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ))
        .await;
        self.exec(&format!(
            "INSERT INTO works (id, owner_pseud_id, title, visibility, lifecycle, created_at,
                                updated_at, version)
             VALUES ('{WORK}','{PSEUD}','Troped','public','published',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ))
        .await;
        self.exec(&format!(
            "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at)
             VALUES ('{COMFORT}','tag','hurt comfort','hurt comfort','2026-01-01T00:00:00Z')"
        ))
        .await;
        self.exec(&format!(
            "INSERT INTO work_tags (work_id, node_id, prominence, added_at)
             VALUES ('{WORK}','{COMFORT}',1,'2026-01-01T00:00:00Z')"
        ))
        .await;
    }

    async fn get(&self, published: bool, path: &str) -> (StatusCode, Value) {
        let response = self
            .client(published)
            .oneshot(
                Request::builder()
                    .uri(path)
                    .body(Body::from(()))
                    .expect("request"),
            )
            .await
            .expect("response");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("body");
        let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, value)
    }
}

#[tokio::test]
async fn the_checklist_answers_404_until_the_operator_publishes_it() {
    let harness = Harness::new("door").await;
    harness.seed().await;

    // A cluster exists, so the only thing standing between the reader and the checklist is the switch.
    harness
        .exec(
            "INSERT INTO trope_clusters (id, slug, label, priority, created_at, updated_at, version)
             VALUES ('01955900-0000-4000-8000-000000000005','hurt-comfort','Hurt/Comfort',10,
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)",
        )
        .await;
    harness
        .exec(
            "INSERT INTO trope_cluster_nodes (cluster_id, node_id)
             VALUES ('01955900-0000-4000-8000-000000000005','01955900-0000-4000-8000-000000000004')",
        )
        .await;

    let (status, _) = harness
        .get(false, &format!("/api/v1/works/{WORK}/trope-coverage"))
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "an unpublically-available checklist is absent, not forbidden"
    );

    let (status, body) = harness
        .get(true, &format!("/api/v1/works/{WORK}/trope-coverage"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let clusters = body["clusters"].as_array().expect("clusters");
    assert_eq!(clusters.len(), 1);
    assert_eq!(clusters[0]["slug"], "hurt-comfort");
    assert_eq!(clusters[0]["missing"], 0, "the work names its only node");
    assert_eq!(body["named_count"], 1);
}
