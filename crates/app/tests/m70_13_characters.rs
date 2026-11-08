//! A work's cast over HTTP (M70-13).
//!
//! The store had 700 lines of tests and no door. This asserts the door: rows written directly (the *write* doors are a
//! separate piece of work) are returned in the shape a page needs, with the relationship's participants folded in rather
//! than left as ids the caller would have to resolve one at a time.

use std::path::PathBuf;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use serde_json::Value;
use test_support::TestDb;
use tower::ServiceExt;

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-m7013c-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

async fn exec(tdb: &TestDb, sql: &str) {
    match tdb.db().backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(sql)
                .execute(tdb.db().sqlite_pool().expect("sqlite pool"))
                .await
                .unwrap_or_else(|error| panic!("sqlite fixture failed: {error}\n{sql}"));
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(sql)
                .execute(tdb.db().postgres_pool().expect("postgres pool"))
                .await
                .unwrap_or_else(|error| panic!("postgres fixture failed: {error}\n{sql}"));
        }
    }
}

async fn get(state: AppState, path: &str) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(Method::GET)
        .uri(path)
        .body(Body::empty())
        .expect("request");
    let response = server::build_router(state)
        .oneshot(request)
        .await
        .expect("response");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("body");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn a_works_cast_and_its_pairings_are_readable() {
    let dir = scratch_dir("cast");
    let tdb = TestDb::connect_with_dir("m7013-cast", &dir).await;
    let mut config = Config::development_defaults();
    config.storage.root = dir.clone();
    let state = AppState::new(config, tdb.db().clone());

    // An account and a pseud, because a work is owned by a pseud (ADR 0003) and the row cannot exist without one.
    exec(
        &tdb,
        "INSERT INTO accounts (id, email, status, age_state, created_at, updated_at, permission_statement)
         VALUES ('0195f000-0000-4000-8000-0000000c00aa', 'cast@example.test', 'active', 'adult',
                 '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 'minimal')",
    )
    .await;
    exec(
        &tdb,
        "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at)
         VALUES ('0195f000-0000-4000-8000-0000000c00bb', '0195f000-0000-4000-8000-0000000c00aa',
                 'castwriter', 'Cast Writer', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
    )
    .await;
    let work = "0195f000-0000-4000-8000-0000000c0001";
    // A work row the read can hang characters off. Only the columns the schema requires are set.
    exec(
        &tdb,
        &format!(
            "INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at) \
             VALUES ('{work}', '0195f000-0000-4000-8000-0000000c00bb', 'A Cast', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        ),
    )
    .await;

    // The node rows the characters and the pairing must point at: `character_node_id` and `relationship_node_id` are both
    // foreign keys into `canonical_entities` (§6.4's kind list).
    for (node, kind) in [
        ("char-a", "character"),
        ("char-b", "character"),
        ("ship-node", "relationship"),
    ] {
        exec(
            &tdb,
            &format!(
                "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at, review_status, signal_count) \
                 VALUES ('{node}', '{kind}', '{node}', '{node}', '2026-01-01T00:00:00Z', 'curated', 0)"
            ),
        )
        .await;
    }

    for (character, prominence, pov) in [("char-a", "protagonist", 1), ("char-b", "supporting", 0)]
    {
        // `1`/`0` rather than `TRUE`/`FALSE`: `is_pov` is an INTEGER on SQLite and a BIGINT on PostgreSQL, and a boolean
        // literal is refused by the second engine while the first accepts it — the dialect trap this project keeps a test
        // for.
        let pov = if pov == 1 { 1 } else { 0 };
        exec(
            &tdb,
            &format!(
                "INSERT INTO work_characters (work_id, character_node_id, prominence, is_pov, added_at) \
                 VALUES ('{work}', '{character}', '{prominence}', {pov}, '2026-01-01T00:00:00Z')"
            ),
        )
        .await;
    }
    let relationship = "0195f000-0000-4000-8000-0000000c0002";
    exec(
        &tdb,
        &format!(
            "INSERT INTO work_relationships (id, work_id, relationship_node_id, rel_type, prominence, added_at) \
             VALUES ('{relationship}', '{work}', 'ship-node', 'romantic', 'secondary', '2026-01-01T00:00:00Z')"
        ),
    )
    .await;
    for participant in ["char-a", "char-b"] {
        exec(
            &tdb,
            &format!(
                "INSERT INTO relationship_participants (relationship_node_id, character_node_id) \
                 VALUES ('ship-node', '{participant}')"
            ),
        )
        .await;
    }

    let (status, body) = get(state, &format!("/api/v1/works/{work}/characters")).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let characters = body["characters"].as_array().expect("characters");
    assert_eq!(characters.len(), 2, "{body}");
    assert_eq!(
        characters[0]["character_node_id"], "char-a",
        "the protagonist is listed first, which is the store's order and not a client's sort: {body}"
    );
    assert_eq!(characters[0]["is_pov"], Value::Bool(true), "{body}");

    let relationships = body["relationships"].as_array().expect("relationships");
    assert_eq!(relationships.len(), 1, "{body}");
    assert_eq!(relationships[0]["rel_type"], "romantic", "{body}");
    assert_eq!(
        relationships[0]["participants"],
        serde_json::json!(["char-a", "char-b"]),
        "the pairing arrives with its ends, not as an id to resolve: {body}"
    );
}
