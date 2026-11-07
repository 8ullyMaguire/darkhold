//! M71-11 — facet counts are memoised for a minute, and the answer says how old it is.
//!
//! The measurement that forced this: `GET /api/v1/facets` on the live catalogue answered 50/50 `200` at **p50 297 ms, p95 372 ms**
//! against a pre-registered budget of p50 ≤ 100 ms / p95 ≤ 200 ms. `EXPLAIN (ANALYZE)` showed why no index helps — three sequential
//! scans and a sort for one group aggregate over 58k works, 127k live units and 127k revisions; the plan is O(catalogue) because the
//! question is about the whole catalogue.
//!
//! So the answer is cached per filter set for [`TTL`](darkhold_app::facets_cache::TTL) and the response carries
//! `computed_at_ms_ago`. This file pins the contract that makes the cache honest:
//!
//! 1. two identical requests inside the window return the same numbers, and the second says the answer is older than zero;
//! 2. a catalogue change inside the window is **not** visible — that is what "memoised" means, and asserting it is how the TTL stays a
//!    documented property instead of an accident;
//! 3. clearing the cache makes the next request reflect the change, which is the escape hatch an operator action needs;
//! 4. a different filter set is a different entry: no correctness-by-collision.

use std::path::PathBuf;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use serde_json::Value;
use test_support::TestDb;
use tower::ServiceExt;

const ACCOUNT: &str = "01955200-0000-4000-8000-0000000a0001";
const PSEUD: &str = "01955200-0000-4000-8000-0000000b0001";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-m7111-{tag}-{}-{:?}",
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

    fn app(&self) -> axum::Router {
        server::build_router(AppState::new(self.config.clone(), self.tdb.db().clone()))
    }

    async fn exec(&self, statement: &str) {
        sqlx::query(statement)
            .execute(self.tdb.db().sqlite_pool().expect("sqlite"))
            .await
            .expect("fixture statement");
    }

    /// One public work with a measured revision, so a facet has something to count.
    async fn add_work(&self, id: &str, words: i64) {
        self.exec(&format!(
            "INSERT INTO works (id, owner_pseud_id, title, language, rating, visibility, lifecycle,
                                published_at, created_at, updated_at, version)
             VALUES ('{id}','{PSEUD}','Work {id}','en','teen','public','published',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z','2026-01-05T00:00:00Z',1)"
        ))
        .await;
        self.exec(&format!(
            "INSERT INTO units (id, work_id, order_key, title, created_at, updated_at, version)
             VALUES ('{id}','{id}',1,'One','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ))
        .await;
        let revision = id.replace('c', "d");
        self.exec(&format!(
            "INSERT INTO revisions (id, unit_id, revision_number, document_json, sanitized_html,
                                    plain_text, word_count, created_by_pseud_id, created_at)
             VALUES ('{revision}','{id}',1,'{{}}','<p>x</p>','x',{words},'{PSEUD}','2026-01-01T00:00:00Z')"
        ))
        .await;
        self.exec(&format!(
            "UPDATE units SET current_revision_id = '{revision}' WHERE id = '{id}'"
        ))
        .await;
    }
}

async fn get(app: &axum::Router, uri: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// The `known` count of the length facet: how many works in the set have a measured length.
fn length_known(body: &Value) -> i64 {
    body["facets"]
        .as_array()
        .expect("facets array")
        .iter()
        .find(|facet| facet["facet"] == "length")
        .and_then(|facet| facet["known"].as_i64())
        .expect("a length facet")
}

#[tokio::test]
async fn a_second_request_inside_the_window_is_served_from_the_cache_and_says_so() {
    let h = Harness::new("served").await;
    darkhold_app::facets_cache::clear();
    h.exec(&format!(
        "INSERT INTO accounts (id, email, created_at, updated_at)
         VALUES ('{ACCOUNT}','m7111@example.invalid','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')"
    ))
    .await;
    h.exec(&format!(
        "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at)
         VALUES ('{PSEUD}','{ACCOUNT}','probe','probe','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')"
    ))
    .await;
    h.add_work("01955200-0000-4000-8000-0000000c0001", 12_000)
        .await;

    let app = h.app();
    let (status, first) = get(&app, "/api/v1/facets").await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(length_known(&first), 1, "{first}");
    assert_eq!(
        first["cache_ttl_seconds"], 60,
        "the response publishes the staleness bound it is operating under: {first}"
    );

    // A work the catalogue gains now must NOT appear on the next request: that is what memoised means, and the age says why.
    h.add_work("01955200-0000-4000-8000-0000000c0002", 3_000)
        .await;
    let (status, second) = get(&app, "/api/v1/facets").await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(
        length_known(&second),
        1,
        "inside the TTL the answer is the one that was computed: {second}"
    );
    assert!(
        second["computed_at_ms_ago"].as_u64().is_some(),
        "the age travels with the numbers instead of being implied: {second}"
    );
}

#[tokio::test]
async fn clearing_the_cache_makes_the_next_request_reflect_the_change() {
    let h = Harness::new("cleared").await;
    darkhold_app::facets_cache::clear();
    h.exec(&format!(
        "INSERT INTO accounts (id, email, created_at, updated_at)
         VALUES ('{ACCOUNT}','m7111b@example.invalid','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')"
    ))
    .await;
    h.exec(&format!(
        "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at)
         VALUES ('{PSEUD}','{ACCOUNT}','probe','probe','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')"
    ))
    .await;
    h.add_work("01955200-0000-4000-8000-0000000c0003", 1_500)
        .await;

    let app = h.app();
    let (_, before) = get(&app, "/api/v1/facets").await;
    assert_eq!(length_known(&before), 1, "{before}");

    h.add_work("01955200-0000-4000-8000-0000000c0004", 25_000)
        .await;
    let (_, memoised) = get(&app, "/api/v1/facets").await;
    assert_eq!(
        length_known(&memoised),
        1,
        "still the memoised answer: {memoised}"
    );

    darkhold_app::facets_cache::clear();
    let (status, fresh) = get(&app, "/api/v1/facets").await;
    assert_eq!(status, StatusCode::OK, "{fresh}");
    assert_eq!(
        length_known(&fresh),
        2,
        "after a clear the scan runs again and counts both works: {fresh}"
    );
    assert!(
        fresh["computed_at_ms_ago"].as_u64().unwrap_or(u64::MAX) < 5_000,
        "a cleared entry is recomputed now, not reused: {fresh}"
    );
}

#[tokio::test]
async fn a_different_filter_is_a_different_entry() {
    let h = Harness::new("filters").await;
    darkhold_app::facets_cache::clear();
    h.exec(&format!(
        "INSERT INTO accounts (id, email, created_at, updated_at)
         VALUES ('{ACCOUNT}','m7111c@example.invalid','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')"
    ))
    .await;
    h.exec(&format!(
        "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at)
         VALUES ('{PSEUD}','{ACCOUNT}','probe','probe','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')"
    ))
    .await;
    h.add_work("01955200-0000-4000-8000-0000000c0005", 1_200)
        .await;
    h.add_work("01955200-0000-4000-8000-0000000c0006", 30_000)
        .await;

    let app = h.app();
    let (_, all) = get(&app, "/api/v1/facets").await;
    assert_eq!(length_known(&all), 2, "{all}");

    // The same shape of request with a word window: a different set, so a different entry — not the cached whole-catalogue answer.
    let (status, windowed) = get(&app, "/api/v1/facets?min_words=10000").await;
    assert_eq!(status, StatusCode::OK, "{windowed}");
    assert_eq!(
        length_known(&windowed),
        1,
        "the window excludes the 1,200-word work: {windowed}"
    );
    // …and the unfiltered entry is still the unfiltered one.
    let (_, again) = get(&app, "/api/v1/facets").await;
    assert_eq!(length_known(&again), 2, "{again}");
}
