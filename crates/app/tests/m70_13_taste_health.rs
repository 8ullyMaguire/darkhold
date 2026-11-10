//! Taste health (§0.4.1, §16.19, M17 phase 2) — the module's doors (M70-13).
//!
//! Five functions had no caller: the per-work vector's producer, writer and reader, plus the two halves of the taste probe.
//! The consequence was not cosmetic — `work_taste_vectors` was a table nothing could fill, so every reader of a vector got
//! `[]`, and a stored vector that is never written reads exactly like a work nobody has taste in common with.
//!
//! The claims:
//!
//! 1. **No profile is not "no vector".** With no admin taste profile the door answers `state: "unavailable"` and says why;
//!    a bare `[]` would be a claim about the work that nobody made.
//! 2. **The read computes once and then serves what is stored** — `computed` then `cached`.
//! 3. **A stale vector is served, not silently rebuilt** (a tag edit does not touch `work_taste_vectors`), and `POST` is
//!    the refresh. That is a decision, so it is asserted rather than hoped for.
//! 4. **The vector is operator-only.** It is a function of the operator's taste profile; enough of them invert it. A reader
//!    with no standing is answered 404, not 403 (ADR 0030).
//! 5. **A probe is one answer per (reader, work)** — the schema's `UNIQUE` and the store's upsert agree — and the
//!    vocabulary is the migration's CHECK, not whatever the client sends.

use axum::http::StatusCode;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::{taste_health, Backend, Database};
use test_support::{register, scratch_dir, sign_in_as, TestClient, TestDb};

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

#[tokio::test]
async fn a_work_vector_is_a_diagnosis_the_read_computes_it_once_and_a_probe_is_one_answer() {
    let dir = scratch_dir("taste-health");
    let tdb = TestDb::connect_with_dir("m7013-taste-health", &dir).await;
    let db: Database = tdb.db().clone();

    // The operator is configured after registering, so the router is rebuilt with the account id and the session is
    // signed in again — the same shape every other admin door's test uses.
    let base = Config::development_defaults();
    let mut bootstrap = TestClient::new(build(&base, &db));
    let operator_account = register(&mut bootstrap, "th-operator@example.test", "thoperator").await;
    let mut config = base.clone();
    config.administration.operator_account_id = Some(operator_account.parse().expect("account id"));
    let mut operator = TestClient::new(build(&config, &db));
    sign_in_as(
        &mut operator,
        &tdb,
        "th-operator@example.test",
        "thoperator",
    )
    .await;

    let mut reader = TestClient::new(build(&config, &db));
    register(&mut reader, "th-reader@example.test", "threader").await;

    // A work with two tagged dimensions, owned by the reader's pseud.
    let (_, me) = reader.get("/api/v1/auth/me").await;
    let reader_account = me["account"]["id"].as_str().unwrap_or_default().to_owned();
    let pseud = test_support::active_pseud_id(&mut reader, &reader_account)
        .await
        .expect("reader pseud");
    let work = "aaaaaaaa-1111-4111-8111-111111111111";
    exec(
        &tdb,
        "INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at) VALUES (?, ?, 'A Tagged Work', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        "INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at) VALUES ($1::uuid, $2::uuid, 'A Tagged Work', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        &[work, &pseud],
    )
    .await;
    // `canonical_entities.id` and `work_tags.node_id` are TEXT on both engines (0011 + 0124) — only `work_tags.work_id`
    // is UUID in PostgreSQL, which is why the cast sits on that parameter and nowhere else.
    for (node, canonical, prominence) in [("n-angst", "Angst", "10"), ("n-fluff", "Fluff", "30")] {
        exec(
            &tdb,
            "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at) VALUES (?, 'tag', ?, ?, '2026-01-01T00:00:00Z')",
            "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at) VALUES ($1, 'tag', $2, $3, '2026-01-01T00:00:00Z')",
            &[node, canonical, &canonical.to_lowercase()],
        )
        .await;
        exec(
            &tdb,
            "INSERT INTO work_tags (work_id, node_id, prominence, added_at) VALUES (?, ?, ?, '2026-01-01T00:00:00Z')",
            "INSERT INTO work_tags (work_id, node_id, prominence, added_at) VALUES ($1::uuid, $2, $3::bigint, '2026-01-01T00:00:00Z')",
            &[work, node, prominence],
        )
        .await;
    }

    // 1. No profile: the door says so rather than deriving an empty vector and calling it a taste.
    let (status, before) = operator
        .get(format!("/api/v1/admin/works/{work}/taste-vector"))
        .await;
    assert_eq!(status, StatusCode::OK, "{before}");
    assert_eq!(
        before["state"],
        serde_json::json!("unavailable"),
        "{before}"
    );
    assert_eq!(before["vector"], serde_json::json!([]), "{before}");
    assert_eq!(before["dimensions"], serde_json::json!(0), "{before}");

    // 2. With a profile, the first read derives and stores — 0.5 base plus prominence/100, keyed on the profile's order
    //    sorted by dimension key: angst 0.5+0.10 = 0.60, fluff 0.5+0.30 = 0.80.
    taste_health::save_admin_taste_profile(
        &db,
        &[
            ("angst".to_string(), "Angst".to_string(), 0.2, 1.0),
            ("fluff".to_string(), "Fluff".to_string(), 0.8, 1.0),
        ],
    )
    .await
    .expect("admin profile");

    let (status, first) = operator
        .get(format!("/api/v1/admin/works/{work}/taste-vector"))
        .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["state"], serde_json::json!("computed"), "{first}");
    assert_eq!(first["dimensions"], serde_json::json!(2), "{first}");
    let vector: Vec<f64> = first["vector"]
        .as_array()
        .expect("vector")
        .iter()
        .filter_map(serde_json::Value::as_f64)
        .collect();
    assert_eq!(vector.len(), 2, "{first}");
    assert!((vector[0] - 0.6).abs() < 1e-9, "angst: {first}");
    assert!((vector[1] - 0.8).abs() < 1e-9, "fluff: {first}");

    // 3. The second read serves what is stored, and says which.
    let (status, second) = operator
        .get(format!("/api/v1/admin/works/{work}/taste-vector"))
        .await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["state"], serde_json::json!("cached"), "{second}");

    // 4. A tag edit does not move the stored vector, and that is visible rather than silent: the read still answers the
    //    old numbers, and `POST` is the refresh.
    exec(
        &tdb,
        "UPDATE work_tags SET prominence = 90 WHERE work_id = ? AND node_id = ?",
        "UPDATE work_tags SET prominence = 90 WHERE work_id = $1::uuid AND node_id = $2",
        &[work, "n-fluff"],
    )
    .await;
    let (status, stale) = operator
        .get(format!("/api/v1/admin/works/{work}/taste-vector"))
        .await;
    assert_eq!(status, StatusCode::OK, "{stale}");
    assert_eq!(
        stale["state"],
        serde_json::json!("cached"),
        "a tag edit is not a recompute, and the door must not pretend otherwise: {stale}"
    );
    assert!(
        (stale["vector"][1].as_f64().unwrap_or_default() - 0.8).abs() < 1e-9,
        "the stale vector is served as stored: {stale}"
    );

    let (status, refreshed) = operator
        .post(
            format!("/api/v1/admin/works/{work}/taste-vector"),
            serde_json::json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{refreshed}");
    assert_eq!(
        refreshed["state"],
        serde_json::json!("computed"),
        "{refreshed}"
    );
    assert!(
        (refreshed["vector"][1].as_f64().unwrap_or_default() - 1.0).abs() < 1e-9,
        "0.5 + 0.9 clamps to 1.0: {refreshed}"
    );

    // 5. A reader with no standing is answered 404 — the vector is a function of the operator's profile, and the door's
    //    existence is not the reader's business (ADR 0030).
    let (status, body) = reader
        .get(format!("/api/v1/admin/works/{work}/taste-vector"))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, body) = reader
        .post(
            format!("/api/v1/admin/works/{work}/taste-vector"),
            serde_json::json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // 6. A probe is recorded, then replaced rather than stacked.
    let (status, body) = reader
        .post(
            format!("/api/v1/me/taste/probes/{work}"),
            serde_json::json!({"engagement": "positive"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["engagement"], serde_json::json!("positive"), "{body}");

    let (status, listed) = reader.get("/api/v1/me/taste/probes").await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert_eq!(listed["count"], serde_json::json!(1), "{listed}");
    assert_eq!(
        listed["probes"][0]["engagement"],
        serde_json::json!("positive"),
        "{listed}"
    );

    let (status, body) = reader
        .post(
            format!("/api/v1/me/taste/probes/{work}"),
            serde_json::json!({"engagement": "negative"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, listed) = reader.get("/api/v1/me/taste/probes").await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert_eq!(
        listed["count"],
        serde_json::json!(1),
        "one answer per (reader, work): {listed}"
    );
    assert_eq!(
        listed["probes"][0]["engagement"],
        serde_json::json!("negative"),
        "the later answer replaces the earlier one: {listed}"
    );

    // 7. A word the table would reject is refused at the door with the field named, not as a 500 from the CHECK.
    let (status, body) = reader
        .post(
            format!("/api/v1/me/taste/probes/{work}"),
            serde_json::json!({"engagement": "loved"}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body.to_string().contains("engagement"), "{body}");

    // 8. Probes are per reader: somebody else's list is empty, not the reader's.
    let mut other = TestClient::new(build(&config, &db));
    register(&mut other, "th-other@example.test", "thother").await;
    let (status, theirs) = other.get("/api/v1/me/taste/probes").await;
    assert_eq!(status, StatusCode::OK, "{theirs}");
    assert_eq!(theirs["count"], serde_json::json!(0), "{theirs}");
}
