//! M64-07 — the crosspost union reaches the feed, ranked once per story.
//!
//! The store's own test proves the grouping arithmetic. This proves the **wiring**: that a feed configured with
//! `["crosspost_union"]` serves the story at its summed strength, once. Without this, the engine is a function with a db test and no
//! caller — the exact shape of "finished" that is not finished.
//!
//! Asserts:
//! 1. one story on two archives appears once in the feed;
//! 2. it outranks a single-archive story with a larger per-copy count, because the union is the sum;
//! 3. the feed carries the summed ordering when the surface is configured to run only this algorithm.

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
        "darkhold-crosspost-feed-{tag}-{}-{:?}",
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
    // One algorithm, because the registry allows a surface to start narrow and the test is about this engine, not the blend.
    config
        .discovery
        .surfaces
        .insert("discover.feed".to_owned(), vec!["crosspost_union".to_owned()]);
    config
}

const ACCOUNT: &str = "01955f00-0000-4000-8000-000000000001";
const PSEUD: &str = "01955f00-0000-4000-8000-000000000002";
const UNION_A: &str = "01955f00-0000-4000-8000-000000000003";
const UNION_B: &str = "01955f00-0000-4000-8000-000000000004";
const SOLO: &str = "01955f00-0000-4000-8000-000000000005";

#[tokio::test]
async fn a_crossposted_story_ranks_once_with_all_of_its_archives() {
    set_trust_proxy(false);
    let dir = scratch_dir("feed");
    let tdb = test_support::TestDb::connect_with_dir("crosspost-feed", &dir).await;
    let db = tdb.db();

    async fn exec(db: &darkhold_db::Database, statement: &str) {
        sqlx::query(statement)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await
            .unwrap_or_else(|error| panic!("{statement}\n{error}"));
    }

    exec(
        db,
        &format!(
            "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version)
             VALUES ('{ACCOUNT}','active','feed@example.invalid','adult',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at,
                                 updated_at, version)
             VALUES ('{PSEUD}','{ACCOUNT}','poster2','Poster','listed',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;

    // Two copies of one story (40 + 25 = 65) and one solo story (50). The union must win on the sum even though the solo copy
    // carries more than either copy alone — that is the difference between ranking copies and ranking stories.
    for (work, title, source, likes) in [
        (UNION_A, "The Long Road", "ao3", 40i64),
        (UNION_B, "the long road", "sb", 25i64),
        (SOLO, "A Solo Story", "ffnet", 50i64),
    ] {
        exec(
            db,
            &format!(
                "INSERT INTO works (id, owner_pseud_id, title, summary, visibility, lifecycle,
                                    created_at, updated_at, version)
                 VALUES ('{work}','{PSEUD}','{title}','','public','published',
                         '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
            ),
        )
        .await;
        exec(
            db,
            &format!(
                "INSERT INTO library_items (id, account_id, work_id, source_key, source_work_key,
                                            title, author_text, summary, status, source_url, likes,
                                            provenance_json, created_at, updated_at, version)
                 VALUES ('01955a10-0000-4000-8000-0000000000{likes:02}','{ACCOUNT}','{work}',
                         '{source}','k{likes}','{title}','Someone','','complete',
                         'https://example.invalid/{likes}',{likes},'{{}}',
                         '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
            ),
        )
        .await;
    }

    let client = server::build_router(AppState::new(config_for(&dir), db.clone()));
    let response = client
        .oneshot(
            Request::builder()
                .uri("/api/v1/discovery")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 22)
        .await
        .expect("body");
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    assert_eq!(status, StatusCode::OK, "{body}");

    let items = body["items"].as_array().expect("items");
    let ids: Vec<&str> = items
        .iter()
        .filter_map(|item| item["work_id"].as_str().or_else(|| item["id"].as_str()))
        .collect();

    // 1. The story appears once, not once per copy.
    let union_rows = ids
        .iter()
        .filter(|id| **id == UNION_A || **id == UNION_B)
        .count();
    assert_eq!(
        union_rows, 1,
        "one story is one row whatever its copies are: {ids:?}"
    );
    assert!(ids.contains(&SOLO), "the solo story is still served: {ids:?}");

    // 2. The representative of the union is first: 65 beats 50.
    assert_eq!(
        ids.first().copied(),
        Some(UNION_A),
        "the summed story leads the page: {ids:?}"
    );

    let _ = std::fs::remove_dir_all(dir);
}
