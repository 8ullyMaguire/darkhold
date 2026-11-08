//! Export idempotency (M70-13): `find_open_export` was written for this and never called.
//!
//! Spec §10.1 is "one job, one artifact". A double click on "export" queued a second job rendering the same file, because
//! nothing asked whether an identical export was already open. The identity is (subject, format): two formats are two
//! different files, and a *finished* export is not open, so asking again after completion still produces a fresh one.

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
                .expect("seed row");
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

    let unit = id(&format!("export-unit-{work}"));
    let revision = id(&format!("export-revision-{work}"));
    exec(
        tdb,
        "INSERT INTO units (id, work_id, title, order_key, created_at, updated_at) VALUES (?, ?, 'Chapter 1', 0, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        "INSERT INTO units (id, work_id, title, order_key, created_at, updated_at) VALUES ($1::uuid, $2::uuid, 'Chapter 1', 0, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        &[&unit, work],
        &[],
    )
    .await;
    // The document is a bound parameter, not a literal: a JSON document inside a Rust string inside SQL is three layers
    // of quoting, and the two engines disagreed about the literal form long before they disagreed about anything else.
    let document = serde_json::json!({"type": "doc", "content": []}).to_string();
    exec(
        tdb,
        "INSERT INTO revisions (id, unit_id, revision_number, document_json, sanitized_html, plain_text, word_count, created_by_pseud_id, created_at)
         VALUES (?, ?, 1, ?, '<p>Hello.</p>', 'Hello. There is a chapter here.', 6, ?, '2026-01-01T00:00:00Z')",
        "INSERT INTO revisions (id, unit_id, revision_number, document_json, sanitized_html, plain_text, word_count, created_by_pseud_id, created_at)
         VALUES ($1::uuid, $2::uuid, 1, $3, '<p>Hello.</p>', 'Hello. There is a chapter here.', 6, $4::uuid, '2026-01-01T00:00:00Z')",
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
async fn asking_twice_for_the_same_export_is_one_export() {
    let dir = scratch_dir("export-idem");
    let tdb = TestDb::connect_with_dir("m7013-export-idem", &dir).await;
    let db: Database = tdb.db().clone();

    let mut config = Config::development_defaults();
    config.storage.root = dir.clone();

    let mut reader = TestClient::new(build(&config, &db));
    let account = register(&mut reader, "export-reader@example.test", "exportreader").await;
    let pseud = active_pseud_id(&mut reader, &account)
        .await
        .expect("active pseud");
    let work = id("export-work");
    seed_work_with_text(&tdb, &work, &pseud).await;

    let request = serde_json::json!({
        "subject_type": "work",
        "subject_id": work,
        "format": "plain_text",
        "acknowledge_privacy": true,
    });

    // 1. First ask queues one export.
    let (status, first) = reader.post("/api/v1/exports", request.clone()).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{first}");
    let first_id = first["id"].as_str().expect("export id").to_owned();

    // 2. The second ask answers with the SAME export rather than a second job.
    let (status, second) = reader.post("/api/v1/exports", request.clone()).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{second}");
    assert_eq!(
        second["id"], serde_json::json!(first_id),
        "a double click is one export, not two: {second}"
    );
    // Counted through the door rather than by SQL: the reader's own list is the view the claim is about, and it keeps
    // this assertion free of the uuid-versus-text placeholder split that the two engines do differently.
    let (status, listed) = reader.get("/api/v1/exports").await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert_eq!(
        listed["exports"].as_array().map(Vec::len),
        Some(1),
        "one job, one artifact (§10.1): {listed}"
    );

    // 3. A finished export is not open, so asking again after completion produces a fresh one.
    exec(
        &tdb,
        "UPDATE export_jobs SET state = 'ready' WHERE id = ?",
        "UPDATE export_jobs SET state = 'ready' WHERE id::text = $1",
        &[&first_id],
        &[],
    )
    .await;
    let (status, third) = reader.post("/api/v1/exports", request.clone()).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{third}");
    assert_ne!(
        third["id"].as_str(),
        Some(first_id.as_str()),
        "a completed export must not be reused as if it were still queued: {third}"
    );

    // 4. Another account asking for the same subject does not get the first account's export.
    let mut other = TestClient::new(build(&config, &db));
    register(&mut other, "export-other@example.test", "exportother").await;
    let (status, theirs) = other.post("/api/v1/exports", request).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{theirs}");
    assert!(
        theirs["id"].as_str() != Some(first_id.as_str()),
        "an export belongs to its account: {theirs}"
    );
}
