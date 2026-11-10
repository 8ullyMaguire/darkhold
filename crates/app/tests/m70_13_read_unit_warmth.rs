//! `ReadUnit` warmth: the last of §41.2's four acts, and the fact it needs (M70-13).
//!
//! Passes 77–78 wired two of §41.2's actions — a kudos (`React`, 200 bp) and finishing a work (`FinishWork`, 500 bp). `ReadUnit`
//! (100 bp) stayed unwired for a reason that is a missing **fact**, not missing glue: `save_progress` stores a position, and a
//! position is overwritten. Nothing in the schema said "this reader has already been credited for this unit", and the one function
//! that tried to be that fact (`reading_goal::record_unit_read`) was deleted in an earlier pass precisely because it was a second,
//! non-transactional writer of progress. Crediting 100 bp per save would pay the reader for scrolling — six saves of the same
//! paragraph would out-earn finishing a work.
//!
//! So the row is a table: `reader_unit_reads`, one row per `(account, unit)`, `INSERT … ON CONFLICT DO NOTHING`, and the credit
//! follows the row that was actually inserted. "Read" is defined by the reader's own fact — their saved position reached the end of
//! the unit (`position_permille = 1000`) — not by a guess about how long the unit is.
//!
//! What this test measures, in basis points read straight from `interaction_warmth`, because a tier is too coarse to catch a
//! double-count:
//!
//! * a save at 500 credits nothing — half a unit is not a unit read;
//! * the first save at 1000 credits 100 bp;
//! * saving the same unit again at 1000 credits **nothing more** (the "already counted" fact is doing its job);
//! * a second unit, completed, takes the reader to 200 bp — the `react` tier — which is also the panel becoming non-trivial.

use axum::http::StatusCode;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::Database;
use serde_json::json;
use test_support::{register, scratch_dir, TestClient, TestDb};

fn build(config: &Config, db: &Database) -> axum::Router {
    server::build_router(AppState::new(config.clone(), db.clone()))
}

async fn account_id(db: &Database, handle: &str) -> uuid::Uuid {
    let sql = db.sql(
        "SELECT account_id FROM pseuds WHERE handle = ?",
        "SELECT account_id::text FROM pseuds WHERE handle = $1",
    );
    let raw: String = match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar(&sql)
            .bind(handle)
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("account for handle"),
        darkhold_db::Backend::Postgres => sqlx::query_scalar(&sql)
            .bind(handle)
            .fetch_one(db.postgres_pool().expect("postgres"))
            .await
            .expect("account for handle"),
    };
    raw.parse().expect("uuid")
}

/// The warmth this author currently has from this reader, in basis points. Zero rows reads as 0.
async fn warmth_bp(db: &Database, reader: uuid::Uuid, author: uuid::Uuid) -> i64 {
    let sql = db.sql(
        "SELECT COALESCE(warmth_bp, 0) FROM interaction_warmth WHERE account_id = ? AND author_account = ?",
        // `warmth_bp` is INT4 on PostgreSQL: decoding it as `i64` is the INT4/BIGINT mismatch that has bitten this repo before, so the
        // column is cast where it is read rather than the reader being loosened.
        "SELECT COALESCE(warmth_bp, 0)::bigint FROM interaction_warmth WHERE account_id = $1::uuid AND author_account = $2::uuid",
    );
    let (r, a) = (reader.to_string(), author.to_string());
    match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar(&sql)
            .bind(&r)
            .bind(&a)
            .fetch_optional(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("warmth")
            .unwrap_or(0),
        darkhold_db::Backend::Postgres => sqlx::query_scalar(&sql)
            .bind(&r)
            .bind(&a)
            .fetch_optional(db.postgres_pool().expect("postgres"))
            .await
            .expect("warmth")
            .unwrap_or(0),
    }
}

async fn publish_work(
    author: &mut TestClient,
    title: &str,
    chapters: usize,
) -> (String, Vec<String>) {
    let (status, work) = author
        .post("/api/v1/works", json!({ "title": title }))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{work}");
    let work_id = work["id"].as_str().expect("work id").to_owned();
    let mut units = Vec::new();
    for index in 0..chapters {
        let (status, unit) = author
            .post(
                &format!("/api/v1/works/{work_id}/chapters"),
                json!({ "title": format!("Unit {index}") }),
            )
            .await;
        assert!(status.is_success(), "unit: {unit}");
        let unit_id = unit["id"].as_str().expect("unit id").to_owned();
        let (status, body) = author
            .request(
                "PATCH",
                &format!("/api/v1/chapters/{unit_id}"),
                Some(json!({
                    "expected_version": unit["version"].as_i64().unwrap_or(1),
                    "document": {
                        "type": "doc",
                        "content": [{
                            "type": "paragraph",
                            "content": [{ "type": "text", "text": "Words a reader can finish." }]
                        }]
                    }
                })),
            )
            .await;
        assert!(status.is_success(), "write unit: {body}");
        units.push(unit_id);
    }
    let (status, current) = author.get(&format!("/api/v1/works/{work_id}")).await;
    assert_eq!(status, StatusCode::OK, "{current}");
    let version = current["version"].as_i64().unwrap_or(1);
    let (status, body) = author
        .post(
            &format!("/api/v1/works/{work_id}/publish"),
            json!({ "expected_version": version }),
        )
        .await;
    assert!(status.is_success(), "publish: {body}");
    (work_id, units)
}

async fn save(reader: &mut TestClient, work: &str, unit: &str, permille: u16) {
    let (status, body) = reader
        .request(
            "PUT",
            "/api/v1/reading/progress",
            Some(json!({
                "subject_type": "work",
                "subject_id": work,
                "unit_id": unit,
                "position_permille": permille
            })),
        )
        .await;
    assert!(status.is_success(), "save progress: {body}");
}

#[tokio::test]
async fn finishing_a_unit_credits_read_warmth_once() {
    let dir = scratch_dir("read-unit-warmth");
    let tdb = TestDb::connect_with_dir("m7013-read-unit-warmth", &dir).await;
    let db: Database = tdb.db().clone();
    let mut config = Config::development_defaults();
    config.storage.root = dir.clone();

    let mut author = TestClient::new(build(&config, &db));
    register(&mut author, "ru-author@example.test", "ruauthor").await;
    let mut reader = TestClient::new(build(&config, &db));
    register(&mut reader, "ru-reader@example.test", "rureader").await;

    let author_account = account_id(&db, "ruauthor").await;
    let reader_account = account_id(&db, "rureader").await;

    let (work, units) = publish_work(&mut author, "Units", 2).await;

    // 0. Nothing yet.
    assert_eq!(warmth_bp(&db, reader_account, author_account).await, 0);

    // 1. Half a unit is not a unit read: a position inside the text credits nothing.
    save(&mut reader, &work, &units[0], 500).await;
    assert_eq!(
        warmth_bp(&db, reader_account, author_account).await,
        0,
        "a position inside a unit is not the act of reading it"
    );

    // 2. Reaching the end credits `ReadUnit` (100 bp).
    save(&mut reader, &work, &units[0], 1000).await;
    assert_eq!(warmth_bp(&db, reader_account, author_account).await, 100);

    // 3. The same unit again credits nothing: the row is the "already counted" fact, and without it six saves of one paragraph would
    //    out-earn finishing a whole work.
    save(&mut reader, &work, &units[0], 1000).await;
    save(&mut reader, &work, &units[0], 1000).await;
    assert_eq!(
        warmth_bp(&db, reader_account, author_account).await,
        100,
        "re-reading a unit is not a second act"
    );

    // 4. A second unit takes the reader to 200 bp, which is the `react` threshold — the panel's first non-`lurk` reader, from reading
    //    alone.
    save(&mut reader, &work, &units[1], 1000).await;
    assert_eq!(warmth_bp(&db, reader_account, author_account).await, 200);
    let (status, panel) = author.get("/api/v1/me/audience").await;
    assert_eq!(status, StatusCode::OK, "{panel}");
    assert_eq!(panel["react"], json!(1), "{panel}");
    assert_eq!(panel["total"], json!(1), "{panel}");
}
