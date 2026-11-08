//! The work-coordinates door (M70-13, giving M45-14's store a surface).
//!
//! Three states are three different facts, and the door must not collapse them: never measured, measured-and-
//! unmeasurable with the reason, and measured with the four numbers plus the text version they describe. It also must
//! not treat an operator as the owner — a stranger gets `404`, because a `403` confirms there is a door to want
//! (ADR 0030).

use axum::http::StatusCode;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::{Backend, Database};
use test_support::{active_pseud_id, id, register, scratch_dir, sign_in_as, TestClient, TestDb};

/// Prose above `MIN_MEASURABLE_WORDS`, with quoted spans so the dialogue ratio is not zero and sentence lengths that
/// vary so the variance is not zero.
fn measurable_prose() -> (String, usize) {
    let mut out = String::new();
    for i in 0..120 {
        out.push_str(&format!(
            "\"Said the {i}th speaker, plainly.\" The night held a quiet patient quality as sentence {i} ended \
             without ornament, and the reader moved through it slowly enough to notice. Short. Then a longer \
             sentence that ran on well past what anyone needed, carrying its clause after clause to the door.\n\n"
        ));
    }
    let words = out.split_whitespace().count();
    (out, words)
}

async fn insert_work(tdb: &TestDb, work_id: &str, pseud: &str) {
    let now = "2026-10-01T12:00:00Z";
    match tdb.db().backend() {
        Backend::Sqlite => {
            sqlx::query("INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at) VALUES (?, ?, ?, ?, ?)")
                .bind(work_id)
                .bind(pseud)
                .bind("A Work For Coordinates")
                .bind(now)
                .bind(now)
                .execute(tdb.db().sqlite_pool().expect("sqlite"))
                .await
                .expect("work row");
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at) \
                 VALUES ($1::uuid, $2::uuid, $3, $4, $5)",
            )
            .bind(work_id)
            .bind(pseud)
            .bind("A Work For Coordinates")
            .bind(now)
            .bind(now)
            .execute(tdb.db().postgres_pool().expect("postgres"))
            .await
            .expect("work row");
        }
    }
}

async fn insert_text(tdb: &TestDb, work_id: &str, pseud: &str, text: &str, words: usize) {
    // Uuid columns on PostgreSQL: an id built by concatenation decodes on SQLite and fails `22P02` on the other
    // engine, which is the dialect trap this suite exists to run twice for.
    let unit = id(&format!("coords-unit-{work_id}"));
    let revision = id(&format!("coords-revision-{work_id}"));
    let now = "2026-10-01T12:00:00Z";
    match tdb.db().backend() {
        Backend::Sqlite => {
            let pool = tdb.db().sqlite_pool().expect("sqlite");
            sqlx::query("INSERT INTO units (id, work_id, title, order_key, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)")
                .bind(&unit)
                .bind(work_id)
                .bind("Chapter 1")
                .bind(0i64)
                .bind(now)
                .bind(now)
                .execute(pool)
                .await
                .expect("unit row");
            sqlx::query(
                "INSERT INTO revisions (id, unit_id, revision_number, document_json, sanitized_html, plain_text, \
                 word_count, created_by_pseud_id, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&revision)
            .bind(&unit)
            .bind(1i64)
            .bind("{}")
            .bind("<p>text</p>")
            .bind(text)
            .bind(words as i64)
            .bind(pseud)
            .bind(now)
            .execute(pool)
            .await
            .expect("revision row");
            sqlx::query("UPDATE units SET current_revision_id = ? WHERE id = ?")
                .bind(&revision)
                .bind(&unit)
                .execute(pool)
                .await
                .expect("pin revision");
        }
        Backend::Postgres => {
            let pool = tdb.db().postgres_pool().expect("postgres");
            sqlx::query("INSERT INTO units (id, work_id, title, order_key, created_at, updated_at) VALUES ($1::uuid, $2::uuid, $3, $4, $5, $6)")
                .bind(&unit)
                .bind(work_id)
                .bind("Chapter 1")
                .bind(0i64)
                .bind(now)
                .bind(now)
                .execute(pool)
                .await
                .expect("unit row");
            sqlx::query(
                "INSERT INTO revisions (id, unit_id, revision_number, document_json, sanitized_html, plain_text, \
                 word_count, created_by_pseud_id, created_at) \
                 VALUES ($1::uuid, $2::uuid, $3, $4, $5, $6, $7, $8::uuid, $9)",
            )
            .bind(&revision)
            .bind(&unit)
            .bind(1i64)
            .bind("{}")
            .bind("<p>text</p>")
            .bind(text)
            .bind(words as i64)
            .bind(pseud)
            .bind(now)
            .execute(pool)
            .await
            .expect("revision row");
            sqlx::query("UPDATE units SET current_revision_id = $1::uuid WHERE id = $2::uuid")
                .bind(&revision)
                .bind(&unit)
                .execute(pool)
                .await
                .expect("pin revision");
        }
    }
}

fn build(config: &Config, db: &Database) -> axum::Router {
    server::build_router(AppState::new(config.clone(), db.clone()))
}

#[tokio::test]
async fn the_door_answers_three_states_and_refuses_a_stranger() {
    let dir = scratch_dir("coords");
    let tdb = TestDb::connect_with_dir("m7013-coords", &dir).await;
    let db: Database = tdb.db().clone();

    let mut config = Config::development_defaults();
    config.storage.root = dir.clone();

    // The writer, through the real registration route so the session and CSRF token are the app's own.
    let mut writer = TestClient::new(build(&config, &db));
    let account = register(&mut writer, "coordinates-writer@example.test", "coordswriter").await;
    let pseud = active_pseud_id(&mut writer, &account)
        .await
        .expect("a registered account acts as a pseud");

    let (prose, words) = measurable_prose();
    let work = id("coords-work");
    insert_work(&tdb, &work, &pseud).await;
    insert_text(&tdb, &work, &pseud, &prose, words).await;

    // 1. Never measured. The absence is a state with a reason, not a zero and not a 404.
    let (status, body) = writer
        .get(format!("/api/v1/works/{work}/coordinates"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["measured"], serde_json::json!(false), "{body}");
    assert_eq!(body["reason"], serde_json::json!("never measured"), "{body}");
    assert!(body["coordinates"].is_null(), "{body}");

    // 2. A stranger. `404`, not `403`: the work's existence is public but whether it is yours is not something a
    //    refusal should confirm.
    let mut stranger = TestClient::new(build(&config, &db));
    register(&mut stranger, "coordinates-stranger@example.test", "coordsstranger").await;
    let (status, body) = stranger
        .get(format!("/api/v1/works/{work}/coordinates"))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "a stranger must not learn this: {body}");

    // 3. Not the operator either: the compute door is closed to the writer who owns the work.
    let (status, body) = writer
        .post(format!("/api/v1/admin/works/{work}/coordinates"), serde_json::json!({}))
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "an owner is not an operator: {body}"
    );

    // 4. The operator recomputes, and the answer is the document a caller will see. A separate account, signed in
    //    through a build that knows it is the operator — an owner reading their own coordinates is not the same claim
    //    as an operator recomputing them.
    let mut op_probe = TestClient::new(build(&config, &db));
    let op_account = register(&mut op_probe, "coordinates-operator@example.test", "coordsoperator").await;
    let mut op_config = config.clone();
    op_config.administration.operator_account_id = Some(op_account.parse().expect("account id"));
    let mut operator = TestClient::new(build(&op_config, &db));
    sign_in_as(&mut operator, &tdb, "coordinates-operator@example.test", "coordsoperator").await;
    let (status, body) = operator
        .post(format!("/api/v1/admin/works/{work}/coordinates"), serde_json::json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["measured"], serde_json::json!(true), "{body}");
    assert_eq!(
        body["coordinates"]["word_count"].as_u64(),
        Some(words as u64),
        "the measures describe the text that was seeded: {body}"
    );
    // The counts are the ones `stored_coordinates` used to hardcode to zero: a 5,880-word work read back as "measured,
    // 0 words", which is the value §49.3 says an absent measurement must never be.
    assert_eq!(
        body["coordinates"]["sentence_count"].as_u64(),
        Some(480),
        "the seeded prose is 480 sentences: {body}"
    );
    assert!(
        body["coordinates"]["vocabulary_richness"].as_f64().is_some_and(|v| v > 0.0),
        "{body}"
    );
    assert!(body["text_version"].as_i64().is_some(), "{body}");
    assert!(body["computed_at"].as_str().is_some(), "{body}");

    // 5. The owner reads back the same numbers the operator wrote — one document, two audiences.
    let (status, body) = writer
        .get(format!("/api/v1/works/{work}/coordinates"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["measured"], serde_json::json!(true), "{body}");
    assert_eq!(body["coordinates"]["word_count"].as_u64(), Some(words as u64), "{body}");

    // 6. Short work: measured, and *unmeasurable with a reason*. The reason is the whole point — a null reads as
    //    "flat prose", which is a different and wrong claim.
    let short = id("coords-short");
    insert_work(&tdb, &short, &pseud).await;
    insert_text(&tdb, &short, &pseud, "Too short to measure at all.", 6).await;
    let (status, body) = operator
        .post(format!("/api/v1/admin/works/{short}/coordinates"), serde_json::json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["measured"], serde_json::json!(false), "{body}");
    assert_eq!(body["unmeasurable"], serde_json::json!("too_short"), "{body}");
    assert!(
        body["reason"].as_str().is_some_and(|r| r.contains("minimum")),
        "the reason travels with the absence: {body}"
    );
}
