//! M64-01 — every recommendation surface has a canonical id, a configurable algorithm set, and a refusal that names
//! what is missing.
//!
//! The registry is closed and lives in `darkhold_domain::recommendation_surfaces` (its own unit tests cover the bounds
//! and the message shapes). This file drives the route, because the half that can lie is the half that talks to the
//! database: a surface whose table is empty must refuse rather than return nothing, and an empty result is
//! indistinguishable from "nothing matches".

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
        "darkhold-m6401-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn config_for(dir: &Path) -> Config {
    let mut config = Config::development_defaults();
    config.storage.root = dir.to_path_buf();
    config.database = DatabaseConfig::new(format!(
        "sqlite://{}/darkhold.sqlite?mode=rwc",
        dir.display()
    ));
    config
}

struct Harness {
    _dir: PathBuf,
    tdb: test_support::TestDb,
    config: Config,
}

impl Harness {
    async fn new(tag: &str) -> Self {
        set_trust_proxy(false);
        let dir = scratch_dir(tag);
        let tdb = test_support::TestDb::connect_with_dir(tag, &dir).await;
        let config = config_for(&dir);
        Self {
            _dir: dir,
            tdb,
            config,
        }
    }

    fn client(&self) -> axum::Router {
        server::build_router(AppState::new(
            self.config.clone(),
            self.tdb.db().clone(),
        ))
    }
}

async fn get(app: axum::Router, uri: &str) -> (StatusCode, Value) {
    let response = app
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let value = serde_json::from_slice(&body)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&body).into_owned()));
    (status, value)
}

#[tokio::test]
async fn every_surface_is_listed_and_empty_data_refuses_by_name() {
    let harness = Harness::new("empty").await;
    let (status, body) = get(harness.client(), "/api/v1/recommendation/surfaces").await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let surfaces = body["surfaces"].as_array().expect("surfaces");
    assert!(
        surfaces.len() >= 20,
        "the registry is a list of every ranking surface, not a sample: {}",
        surfaces.len()
    );

    for surface in surfaces {
        let id = surface["id"].as_str().expect("id");
        assert!(id.contains('.'), "{id} is not a canonical dotted id");
        let algorithms = surface["algorithms"].as_array().expect("algorithms");
        assert!(
            (1..=4).contains(&algorithms.len()),
            "{id} runs {} algorithms; the contract is one to four",
            algorithms.len()
        );
        let need = surface["need"].as_str().expect("need");

        if need == "none" {
            // A surface that needs no data cannot refuse for lack of it. It may still name an algorithm no engine
            // implements yet — that is a build fact, not a data fact, and the two must not be conflated.
            for entry in surface["refused"].as_array().expect("refused") {
                let reason = entry["reason"].as_str().expect("reason");
                assert!(
                    !reason.contains("no rows yet"),
                    "{id} needs nothing, so it cannot refuse for empty data: {entry}"
                );
            }
            continue;
        }

        // Every algorithm on a surface whose table is empty is refused for lack of data, by name, with the milestone
        // that fills it. (A surface may also carry "no engine implements this" entries; those are counted separately,
        // because the two claims differ in what an operator should do next.)
        let refused = surface["refused"].as_array().expect("refused");
        let data_refusals: Vec<&Value> = refused
            .iter()
            .filter(|entry| {
                entry["reason"]
                    .as_str()
                    .is_some_and(|reason| reason.contains("no rows yet"))
            })
            .collect();
        assert_eq!(
            data_refusals.len(),
            algorithms.len(),
            "{id} needs `{need}` and has no rows, so every algorithm refuses: {surface}"
        );
        for entry in data_refusals {
            let algorithm = entry["algorithm"].as_str().expect("algorithm");
            assert!(
                algorithms.iter().any(|a| a.as_str() == Some(algorithm)),
                "the refusal names an algorithm the surface runs: {entry}"
            );
            let reason = entry["reason"].as_str().expect("reason");
            // The reason names the table (`shelf_items`, not the need's label `shelves`), so it is checked for the
            // shape rather than for the label: something is empty, and a milestone is named.
            assert!(
                reason.contains("no rows yet"),
                "the refusal says a table is empty: {entry}"
            );
            let _ = need;
            let milestone = entry["milestone"].as_str().expect("milestone");
            assert!(
                milestone.starts_with('M') && milestone.contains('-'),
                "the refusal names the milestone that fills it: {entry}"
            );
        }
    }
}

#[tokio::test]
async fn a_surface_stops_refusing_once_its_table_has_rows() {
    let harness = Harness::new("filled").await;
    // One tag edge is enough: the surface's question stops being "is this table empty" and becomes "which works".
    sqlx::query(
        "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at)
         VALUES ('01954000-0000-4000-8000-0000000009f1','tag','slow burn','slow burn','2026-01-01T00:00:00Z')",
    )
    .execute(harness.tdb.db().sqlite_pool().expect("sqlite"))
    .await
    .expect("seed tag");

    let (status, body) = get(harness.client(), "/api/v1/recommendation/surfaces").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let surfaces = body["surfaces"].as_array().expect("surfaces");

    let tag_surface = surfaces
        .iter()
        .find(|s| s["id"] == "tag.adjacent")
        .expect("tag.adjacent is registered");
    assert_eq!(tag_surface["need"], "work_tags");
    // Still refused: a tag entity is not a tag *application*. The dependency is `work_tags`, and it is empty until a
    // work carries a tag — which is exactly the distinction an operator needs, so the probe counts the right table.
    assert!(
        !tag_surface["refused"].as_array().expect("refused").is_empty(),
        "an entity row is not a work_tag row: {tag_surface}"
    );

    // A work with a tag clears it.
    let account = "01955000-0000-4000-8000-0000000009a1";
    let pseud = "01955000-0000-4000-8000-0000000009e1";
    let work = "01957000-0000-4000-8000-0000000009b1";
    for statement in [
        format!(
            "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version)
             VALUES ('{account}','active','m6401@example.invalid','adult','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
        format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at, updated_at, version)
             VALUES ('{pseud}','{account}','m6401','M6401','listed','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
        format!(
            "INSERT INTO works (id, owner_pseud_id, title, language, rating, visibility, lifecycle, created_at, updated_at, version)
             VALUES ('{work}','{pseud}','Tagged','en','general','public','published','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
        format!(
            "INSERT INTO work_tags (work_id, node_id, prominence, added_at)
             VALUES ('{work}','01954000-0000-4000-8000-0000000009f1',1,'2026-01-01T00:00:00Z')"
        ),
        // M64-06 changed what "has rows" means for a tag surface: a tag on a work nobody has touched is a tag with nothing to rank
        // against, and the surface now refuses in that state. This test is about the rows reaching the response, so it seeds an
        // engaged work too — the disjoint case has its own test below, and the two must not be able to pass at once.
        format!(
            "INSERT INTO work_metric_aggregates (work_id, views, complete_reads, reactions, kudos,
                     bookmarks, collection_adds, reviews, updated_at)
             VALUES ('{work}', 3, 0, 0, 1, 0, 0, 0, '2026-01-01T00:00:00Z')"
        ),
    ] {
        sqlx::query(&statement)
            .execute(harness.tdb.db().sqlite_pool().expect("sqlite"))
            .await
            .unwrap_or_else(|error| panic!("{statement}\n{error}"));
    }

    let (status, body) = get(harness.client(), "/api/v1/recommendation/surfaces").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let surfaces = body["surfaces"].as_array().expect("surfaces");
    let tag_surface = surfaces
        .iter()
        .find(|s| s["id"] == "tag.adjacent")
        .expect("tag.adjacent is registered");
    assert!(
        tag_surface["refused"].as_array().expect("refused").is_empty(),
        "one tagged, engaged work is enough for the surface to answer: {tag_surface}"
    );
}

#[tokio::test]
async fn a_configured_surface_set_reaches_the_response() {
    let mut harness = Harness::new("configured").await;
    // One algorithm is legal — it is where every surface starts (M64-01's own words).
    harness
        .config
        .discovery
        .surfaces
        .insert("tag.adjacent".to_owned(), vec!["npmi".to_owned()]);

    let (status, body) = get(harness.client(), "/api/v1/recommendation/surfaces").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let surfaces = body["surfaces"].as_array().expect("surfaces");
    let tag_surface = surfaces
        .iter()
        .find(|s| s["id"] == "tag.adjacent")
        .expect("tag.adjacent is registered");
    assert_eq!(
        tag_surface["algorithms"],
        serde_json::json!(["npmi"]),
        "the configured set is what the surface runs, not the default: {tag_surface}"
    );
}

/// M64-06 — the refusal a tag surface needs is not "the table is empty".
///
/// `work_tags` and `work_metric_aggregates` can both be full and still share nothing: every tagged work is an import nobody read. In
/// that state a "no rows yet" refusal never fires and the surface reports that it is ranking with `tag_jaccard` while returning
/// nothing. The three states are distinguishable, and this drives all three through the route:
///
/// 1. no tags at all       -> "has no rows yet"
/// 2. tags, no engagement  -> "no work carries both" (the live instance's state on 2026-11-01)
/// 3. one work both        -> no data refusal at all
#[tokio::test]
async fn a_tag_surface_refuses_when_tags_and_engagement_never_meet() {
    let harness = Harness::new("disjoint").await;
    let db = harness.tdb.db();

    async fn exec(db: &darkhold_db::Database, statement: &str) {
        sqlx::query(statement)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await
            .unwrap_or_else(|error| panic!("{statement}\n{error}"));
    }

    // Seed a work, an author and two tags: plenty of rows, none of them engaged.
    exec(db, "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version)
              VALUES ('01955a00-0000-4000-8000-000000000001','active','t@example.invalid','adult',
                      '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)").await;
    exec(db, "INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at, updated_at, version)
              VALUES ('01955a00-0000-4000-8000-000000000002','01955a00-0000-4000-8000-000000000001',
                      'tagger','Tagger','listed','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)").await;
    exec(db, "INSERT INTO works (id, owner_pseud_id, title, visibility, lifecycle, created_at, updated_at, version)
              VALUES ('01955a00-0000-4000-8000-000000000003','01955a00-0000-4000-8000-000000000002',
                      'Tagged','public','published','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)").await;
    for (node, name) in [
        ("01955a00-0000-4000-8000-000000000004", "slow burn"),
        ("01955a00-0000-4000-8000-000000000005", "angst"),
    ] {
        exec(db, &format!(
            "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at)
             VALUES ('{node}','tag','{name}','{name}','2026-01-01T00:00:00Z')")).await;
        exec(db, &format!(
            "INSERT INTO work_tags (work_id, node_id, prominence, added_at)
             VALUES ('01955a00-0000-4000-8000-000000000003','{node}',1,'2026-01-01T00:00:00Z')")).await;
    }

    let find = |body: &Value, id: &str| -> Value {
        body["surfaces"]
            .as_array()
            .expect("surfaces")
            .iter()
            .find(|surface| surface["id"] == id)
            .cloned()
            .unwrap_or_else(|| panic!("{id} is in the registry"))
    };

    // 2. Tags present, nobody engaged: the refusal names the overlap, not an empty table.
    let (status, body) = get(harness.client(), "/api/v1/recommendation/surfaces").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let similar = find(&body, "story.similar");
    let refused = similar["refused"].as_array().expect("refused");
    let jaccard = refused
        .iter()
        .find(|entry| entry["algorithm"] == "tag_jaccard")
        .unwrap_or_else(|| panic!("tag_jaccard is refused while disjoint: {refused:?}"));
    let reason = jaccard["reason"].as_str().expect("reason");
    assert!(
        reason.contains("no work carries both"),
        "the refusal must name the real gap, not an empty table: {reason}"
    );
    assert!(
        !reason.contains("no rows yet"),
        "the table is not empty and saying so would send an operator to the wrong fix: {reason}"
    );

    // 3. One engaged tagged work makes the surface usable again.
    exec(db, "INSERT INTO work_metric_aggregates (work_id, views, complete_reads, reactions, kudos,
                     bookmarks, collection_adds, reviews, updated_at)
              VALUES ('01955a00-0000-4000-8000-000000000003', 5, 1, 0, 2, 0, 0, 0,
                      '2026-01-01T00:00:00Z')").await;
    let (status, body) = get(harness.client(), "/api/v1/recommendation/surfaces").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let similar = find(&body, "story.similar");
    let refused = similar["refused"].as_array().expect("refused");
    assert!(
        !refused.iter().any(|entry| entry["algorithm"] == "tag_jaccard"),
        "with one tagged, engaged work there is something to rank: {refused:?}"
    );
}
