//! The export grant list, the bulk item list, and the ownership predicate moved into the query (M70-13).
//!
//! Three functions, three claims:
//!
//! * `grants_for_export` — a reader can see when their download links expire and which have been spent, without the
//!   token ever being echoed back (only its hash is stored, and the hash is not the token either).
//! * `bulk_items_for_export` — a bulk export's detail carries its per-work decisions, which is the export's own audit
//!   trail rather than a claim in the code.
//! * `find_export_for` — the ownership rule now lives in the query, so this test asserts what the *door* promises: a
//!   stranger gets 404 for both the export and its grants.

use axum::http::StatusCode;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::{Backend, Database};
use test_support::{active_pseud_id, id, register, scratch_dir, TestClient, TestDb};

async fn exec(tdb: &TestDb, sqlite: &str, postgres: &str, binds: &[&str], ints: &[i64]) {
    match tdb.db().backend() {
        Backend::Sqlite => {
            let mut q = sqlx::query(sqlite);
            for b in binds {
                q = q.bind(*b);
            }
            for i in ints {
                q = q.bind(*i);
            }
            q.execute(tdb.db().sqlite_pool().expect("sqlite"))
                .await
                .unwrap_or_else(|error| panic!("seed row failed: {error}\nSQL: {sqlite}"));
        }
        Backend::Postgres => {
            let mut q = sqlx::query(postgres);
            for b in binds {
                q = q.bind(*b);
            }
            for i in ints {
                q = q.bind(*i);
            }
            q.execute(tdb.db().postgres_pool().expect("postgres"))
                .await
                .unwrap_or_else(|error| panic!("seed row failed: {error}\nSQL: {postgres}"));
        }
    }
}

fn build(config: &Config, db: &Database) -> axum::Router {
    server::build_router(AppState::new(config.clone(), db.clone()))
}

async fn seed_work_with_text(tdb: &TestDb, work: &str, pseud: &str) {
    exec(
        tdb,
        "INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at) VALUES (?, ?, 'An Exportable Work', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        "INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at) VALUES ($1::uuid, $2::uuid, 'An Exportable Work', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        &[work, pseud],
        &[],
    )
    .await;
    let unit = id(&format!("grants-unit-{work}"));
    let revision = id(&format!("grants-revision-{work}"));
    let document = serde_json::json!({"type": "doc", "content": []}).to_string();
    exec(
        tdb,
        "INSERT INTO units (id, work_id, title, order_key, created_at, updated_at) VALUES (?, ?, 'Chapter 1', 0, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        "INSERT INTO units (id, work_id, title, order_key, created_at, updated_at) VALUES ($1::uuid, $2::uuid, 'Chapter 1', 0, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        &[&unit, work],
        &[],
    )
    .await;
    exec(
        tdb,
        "INSERT INTO revisions (id, unit_id, revision_number, document_json, sanitized_html, plain_text, word_count, created_by_pseud_id, created_at)
         VALUES (?, ?, 1, ?, '<p>Hi.</p>', 'Hi there.', 2, ?, '2026-01-01T00:00:00Z')",
        "INSERT INTO revisions (id, unit_id, revision_number, document_json, sanitized_html, plain_text, word_count, created_by_pseud_id, created_at)
         VALUES ($1::uuid, $2::uuid, 1, $3, '<p>Hi.</p>', 'Hi there.', 2, $4::uuid, '2026-01-01T00:00:00Z')",
        &[&revision, &unit, &document, pseud],
        &[],
    )
    .await;
    exec(
        tdb,
        "UPDATE units SET current_revision_id = ? WHERE id = ?",
        "UPDATE units SET current_revision_id = $1::uuid WHERE id = $2::uuid",
        &[&revision, &unit],
        &[],
    )
    .await;
}

#[tokio::test]
async fn grants_are_listable_without_their_token_and_a_stranger_sees_neither_export_nor_grants() {
    let dir = scratch_dir("export-grants");
    let tdb = TestDb::connect_with_dir("m7013-export-grants", &dir).await;
    let db: Database = tdb.db().clone();

    let mut config = Config::development_defaults();
    config.storage.root = dir.clone();

    let mut reader = TestClient::new(build(&config, &db));
    let account = register(&mut reader, "grants-reader@example.test", "grantsreader").await;
    let pseud = active_pseud_id(&mut reader, &account)
        .await
        .expect("active pseud");
    let work = id("grants-work");
    seed_work_with_text(&tdb, &work, &pseud).await;

    let (status, export) = reader
        .post(
            "/api/v1/exports",
            serde_json::json!({
                "subject_type": "work",
                "subject_id": work,
                "format": "plain_text",
                "acknowledge_privacy": true,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{export}");
    let export_id = export["id"].as_str().expect("export id").to_owned();

    // 1. No grants yet: an empty list, not a 404 — the export exists, its links do not.
    let (status, body) = reader
        .get(format!("/api/v1/exports/{export_id}/grants"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["grants"].as_array().map(Vec::len), Some(0), "{body}");

    // 2. One grant, seeded directly: minting needs a finished blob, and what this test is about is the read.
    let grant = id("grants-grant");
    exec(
        &tdb,
        "INSERT INTO download_grants (id, export_job_id, token_hash, expires_at, used_at, single_use, created_at)
         VALUES (?, ?, 'sha256-of-a-token', '2026-02-01T00:00:00Z', NULL, 1, '2026-01-05T00:00:00Z')",
        "INSERT INTO download_grants (id, export_job_id, token_hash, expires_at, used_at, single_use, created_at)
         VALUES ($1::uuid, $2::uuid, 'sha256-of-a-token', '2026-02-01T00:00:00Z', NULL, 1, '2026-01-05T00:00:00Z')",
        &[&grant, &export_id],
        &[],
    )
    .await;

    let (status, body) = reader
        .get(format!("/api/v1/exports/{export_id}/grants"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let grants = body["grants"].as_array().expect("array");
    assert_eq!(grants.len(), 1, "{body}");
    assert_eq!(
        grants[0]["expires_at"],
        serde_json::json!("2026-02-01T00:00:00Z"),
        "{body}"
    );
    assert!(grants[0]["used_at"].is_null(), "{body}");
    assert!(
        !body.to_string().contains("sha256-of-a-token"),
        "the hash is not the token, and it is not the reader's business either: {body}"
    );

    // 3. A bulk export's detail carries its per-work decisions. The row is switched to `query` rather than run through
    //    the bulk pipeline, because the subject here is the read that was missing, not the walk that fills the table.
    exec(
        &tdb,
        "UPDATE export_jobs SET subject_type = 'query' WHERE id = ?",
        "UPDATE export_jobs SET subject_type = 'query' WHERE id::text = $1",
        &[&export_id],
        &[],
    )
    .await;
    let item = id("grants-bulk-item");
    exec(
        &tdb,
        "INSERT INTO bulk_export_items (id, export_job_id, work_id, decision, reason, blob_checksum, byte_size, created_at, updated_at, version)
         VALUES (?, ?, ?, 'skipped', 'not eligible: no media', NULL, NULL, '2026-01-06T00:00:00Z', '2026-01-06T00:00:00Z', 1)",
        "INSERT INTO bulk_export_items (id, export_job_id, work_id, decision, reason, blob_checksum, byte_size, created_at, updated_at, version)
         VALUES ($1::uuid, $2::uuid, $3::uuid, 'skipped', 'not eligible: no media', NULL, NULL, '2026-01-06T00:00:00Z', '2026-01-06T00:00:00Z', 1)",
        &[&item, &export_id, &work],
        &[],
    )
    .await;
    let (status, body) = reader.get(format!("/api/v1/exports/{export_id}")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["subject_type"], serde_json::json!("query"), "{body}");
    let items = body["items"].as_array().expect("items on a bulk export");
    assert_eq!(items.len(), 1, "{body}");
    assert_eq!(items[0]["decision"], serde_json::json!("skipped"), "{body}");
    assert_eq!(
        items[0]["reason"],
        serde_json::json!("not eligible: no media"),
        "{body}"
    );

    // 4. A plain export's detail has no `items` field at all — the document shape every other export has always had.
    exec(
        &tdb,
        "UPDATE export_jobs SET subject_type = 'work' WHERE id = ?",
        "UPDATE export_jobs SET subject_type = 'work' WHERE id::text = $1",
        &[&export_id],
        &[],
    )
    .await;
    let (status, body) = reader.get(format!("/api/v1/exports/{export_id}")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.get("items").is_none(), "{body}");

    // 5. A stranger gets 404 from both doors — the ownership predicate is in the query now, and this is the assertion
    //    that it still refuses.
    let mut stranger = TestClient::new(build(&config, &db));
    register(
        &mut stranger,
        "grants-stranger@example.test",
        "grantsstranger",
    )
    .await;
    let (status, body) = stranger.get(format!("/api/v1/exports/{export_id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, body) = stranger
        .get(format!("/api/v1/exports/{export_id}/grants"))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}
