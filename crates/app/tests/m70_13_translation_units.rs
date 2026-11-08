//! M17 translation units, publication, and memory (M70-13): five store functions that had no caller.
//!
//! * `units_for_job` — a job in progress is a list of paragraphs with their own states; a caller that only saw the job's
//!   state could not tell three approved paragraphs from seven pending ones.
//! * `upsert_unit` — recording a paragraph, idempotent on `(job, unit, paragraph)`.
//! * `record_publication` — the row that says a translation reached a reader, written only when the job actually moved to
//!   `published`, because a publication row for a job that never moved is a claim about an event that did not happen.
//! * `add_memory` / `lookup_memory` — the write and the match beside the existing list, keyed on one hashing of the source
//!   sentence that the writer and the reader share (`memory_key`), so a stored pair can always find itself.
//!
//! It also closes a leak the doors would have inherited: `get_translation` answered anybody with a job id, and a
//! translation job carries the provider, the quote and the language pair of somebody's work.

use axum::http::StatusCode;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::{Backend, Database};
use test_support::{active_pseud_id, id, register, scratch_dir, TestClient, TestDb};

fn build(config: &Config, db: &Database) -> axum::Router {
    server::build_router(AppState::new(config.clone(), db.clone()))
}

async fn exec(tdb: &TestDb, sqlite: &str, postgres: &str, binds: &[&str]) {
    match tdb.db().backend() {
        Backend::Sqlite => {
            let mut q = sqlx::query(sqlite);
            for b in binds {
                q = q.bind(*b);
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
            q.execute(tdb.db().postgres_pool().expect("postgres"))
                .await
                .unwrap_or_else(|error| panic!("seed row failed: {error}\nSQL: {postgres}"));
        }
    }
}

async fn seed_job(tdb: &TestDb, job: &str, work: &str, account: &str, state: &str) {
    exec(
        tdb,
        "INSERT INTO translation_jobs (id, source_work, source_lang, target_lang, provider, quote_transaction, state, created_by, created_at, updated_at)
         VALUES (?, ?, 'en', 'es', 'a-provider', NULL, ?, ?, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        "INSERT INTO translation_jobs (id, source_work, source_lang, target_lang, provider, quote_transaction, state, created_by, created_at, updated_at)
         VALUES ($1::uuid, $2::uuid, 'en', 'es', 'a-provider', NULL, $3, $4::uuid, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        &[job, work, state, account],
    )
    .await;
}

#[tokio::test]
async fn a_job_is_its_owners_its_units_are_per_paragraph_and_a_publication_needs_approval() {
    let dir = scratch_dir("translation-units");
    let tdb = TestDb::connect_with_dir("m7013-translation-units", &dir).await;
    let db: Database = tdb.db().clone();

    let config = Config::development_defaults();

    let mut owner = TestClient::new(build(&config, &db));
    let account = register(&mut owner, "tr-owner@example.test", "trowner").await;
    let pseud = active_pseud_id(&mut owner, &account)
        .await
        .expect("owner pseud");
    let work = id("tr-work");
    exec(
        &tdb,
        "INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at) VALUES (?, ?, 'A Work To Translate', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        "INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at) VALUES ($1::uuid, $2::uuid, 'A Work To Translate', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        &[&work, &pseud],
    )
    .await;

    let job = id("tr-job");
    seed_job(&tdb, &job, &work, &account, "in_progress").await;
    let unit = id("tr-unit");

    // 1. The owner reads their job; a stranger gets 404, which is the leak this pass closed.
    let (status, body) = owner.get(format!("/api/v1/translations/{job}")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let mut stranger = TestClient::new(build(&config, &db));
    register(&mut stranger, "tr-stranger@example.test", "trstranger").await;
    // An active pseud so the request reaches the handler rather than stopping at the extractor.
    let stranger_account = {
        let (_, me) = stranger.get("/api/v1/auth/me").await;
        me["account"]["id"].as_str().unwrap_or_default().to_owned()
    };
    let _ = stranger_account;
    let (status, body) = stranger.get(format!("/api/v1/translations/{job}")).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a job id is not a capability: {body}"
    );

    // 2. Two paragraphs recorded, one translated and one still pending, and the list keeps them apart.
    let (status, body) = owner
        .post(
            format!("/api/v1/translations/{job}/units"),
            serde_json::json!({
                "unit_id": unit,
                "paragraph_index": 0,
                "source_text": "The sea was grey that morning.",
                "target_text": "El mar estaba gris esa mañana.",
                "state": "translated",
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = owner
        .post(
            format!("/api/v1/translations/{job}/units"),
            serde_json::json!({
                "unit_id": unit,
                "paragraph_index": 1,
                "source_text": "Nobody spoke.",
                "state": "pending",
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, listed) = owner
        .get(format!("/api/v1/translations/{job}/units"))
        .await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    let units = listed["units"].as_array().expect("units");
    assert_eq!(units.len(), 2, "{listed}");

    // 3. Recording the same paragraph twice updates it rather than adding a third: the key is (job, unit, paragraph).
    let (status, body) = owner
        .post(
            format!("/api/v1/translations/{job}/units"),
            serde_json::json!({
                "unit_id": unit,
                "paragraph_index": 0,
                "source_text": "The sea was grey that morning.",
                "target_text": "El mar era gris aquella mañana.",
                "state": "reviewed",
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, listed) = owner
        .get(format!("/api/v1/translations/{job}/units"))
        .await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert_eq!(
        listed["units"].as_array().map(Vec::len),
        Some(2),
        "an upsert, not an append: {listed}"
    );

    // 4. A paragraph cannot be "translated" with nothing in it.
    let (status, body) = owner
        .post(
            format!("/api/v1/translations/{job}/units"),
            serde_json::json!({
                "unit_id": unit,
                "paragraph_index": 2,
                "source_text": "A third sentence.",
                "state": "translated",
            }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    // ... and an unknown state is a validation error naming the field.
    let (status, body) = owner
        .post(
            format!("/api/v1/translations/{job}/units"),
            serde_json::json!({
                "unit_id": unit,
                "paragraph_index": 3,
                "source_text": "A fourth sentence.",
                "target_text": "Una cuarta frase.",
                "state": "shipped",
            }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    // 5. Publication is refused while the job is in progress.
    let (status, body) = owner
        .post(format!("/api/v1/translations/{job}/publish"), serde_json::json!({}))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body.to_string().contains("approved"),
        "the refusal says which state it would accept: {body}"
    );

    // 6. From `approved` it publishes, records the publication, and refuses a second time.
    exec(
        &tdb,
        "UPDATE translation_jobs SET state = 'approved' WHERE id = ?",
        "UPDATE translation_jobs SET state = 'approved' WHERE id::text = $1",
        &[&job],
    )
    .await;
    let (status, body) = owner
        .post(format!("/api/v1/translations/{job}/publish"), serde_json::json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["publication_id"].as_str().is_some_and(|id| !id.is_empty()),
        "a publication row is written: {body}"
    );
    assert_eq!(body["state"], serde_json::json!("published"), "{body}");
    let (status, body) = owner.get(format!("/api/v1/translations/{job}")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["state"], serde_json::json!("published"), "{body}");
    let (status, body) = owner
        .post(format!("/api/v1/translations/{job}/publish"), serde_json::json!({}))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    // 7. Memory: write a pair, then find the same sentence through a lookup that trims. The key is one hashing, shared.
    let (status, body) = owner
        .post(
            "/api/v1/me/translation-memory",
            serde_json::json!({
                "source_lang": "en",
                "target_lang": "es",
                "source_text": "The sea was grey that morning.",
                "target_text": "El mar estaba gris esa mañana.",
                "quality_bp": 9500,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let hash = body["source_hash"].as_str().expect("hash").to_owned();
    assert_eq!(hash.len(), 64, "hex sha256: {body}");

    // Encoded, with the padding the test is about: leading and trailing spaces must not change the key.
    let (status, hit) = owner
        .get("/api/v1/me/translation-memory/lookup?source_lang=en&target_lang=es&source_text=%20%20The%20sea%20was%20grey%20that%20morning.%20%20")
        .await;
    assert_eq!(status, StatusCode::OK, "{hit}");
    assert_eq!(
        hit["target_text"],
        serde_json::json!("El mar estaba gris esa mañana."),
        "the writer and the reader agree on the key: {hit}"
    );
    assert_eq!(hit["quality_bp"], serde_json::json!(9500), "{hit}");

    // 8. A sentence nobody has translated answers null rather than an empty object.
    let (status, miss) = owner
        .get("/api/v1/me/translation-memory/lookup?source_lang=en&target_lang=es&source_text=Something%20else%20entirely.")
        .await;
    assert_eq!(status, StatusCode::OK, "{miss}");
    assert!(miss["target_text"].is_null(), "{miss}");

    // 9. Memory is per account: a second reader's lookup of the same sentence finds nothing.
    let (status, theirs) = stranger
        .get("/api/v1/me/translation-memory/lookup?source_lang=en&target_lang=es&source_text=The%20sea%20was%20grey%20that%20morning.")
        .await;
    assert_eq!(status, StatusCode::OK, "{theirs}");
    assert!(
        theirs["target_text"].is_null(),
        "another reader's memory is not yours: {theirs}"
    );
}
