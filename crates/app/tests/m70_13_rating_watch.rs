//! M28 §33.2 — the anomaly queue gets its producer, its listing, and the contested mark (M70-13).
//!
//! Before this pass `rating_anomaly_events` had a reader that faked its answer (`{count: n, items: []}`), a clearer an
//! operator could call, and a writer nothing ever called. A queue with no producer reads exactly like a healthy instance:
//! "0 anomalies" forever. Five store functions had no caller and now do:
//!
//! * `insert_rating_anomaly_event` — the writer, called by `rating_watch::detect_bursts`.
//! * `get_work_anomaly_events` — one work's events, for its moderation page.
//! * `set_work_contested` / `clear_work_contested` — the quorum's mark, with the reason it requires.
//! * `is_work_contested` — the aggregate door's refusal, which is the reason the mark exists at all.
//!
//! And the listing that used to lie: `/admin/anomalies` now names the works it counts.

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

/// The account id behind a handle, as the type `config.administration` stores.
async fn account_of(db: &Database, handle: &str) -> darkhold_domain::ids::AccountId {
    let sql = match db.backend() {
        Backend::Sqlite => "SELECT account_id FROM pseuds WHERE lower(handle) = lower(?)",
        Backend::Postgres => {
            "SELECT account_id::text AS account_id FROM pseuds WHERE lower(handle) = lower($1)"
        }
    };
    let value: String = match db.backend() {
        Backend::Sqlite => sqlx::query_scalar(sql)
            .bind(handle)
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("the account"),
        Backend::Postgres => sqlx::query_scalar(sql)
            .bind(handle)
            .fetch_one(db.postgres_pool().expect("postgres"))
            .await
            .expect("the account"),
    };
    value.parse().expect("account id parses")
}

#[tokio::test]
async fn a_fresh_account_burst_is_detected_once_listed_and_its_work_can_be_contested() {
    let dir = scratch_dir("rating-watch");
    let tdb = TestDb::connect_with_dir("m7013-rating-watch", &dir).await;
    let db: Database = tdb.db().clone();

    let config = Config::development_defaults();

    // Five freshly registered readers. Their `accounts.created_at` is now, which is what "fresh" means to the detector,
    // and registering them through the real route is the only way to get rows the detector can see.
    let mut pseids = Vec::new();
    let mut author_client = TestClient::new(build(&config, &db));
    let author = register(&mut author_client, "rw-author@example.test", "rwauthor").await;
    let author_pseud = active_pseud_id(&mut author_client, &author)
        .await
        .expect("author pseud");
    pseids.push((author.clone(), author_pseud.clone()));

    for index in 0..4 {
        let mut client = TestClient::new(build(&config, &db));
        let account = register(
            &mut client,
            &format!("rw-fresh{index}@example.test"),
            &format!("rwfresh{index}"),
        )
        .await;
        let pseud = active_pseud_id(&mut client, &account)
            .await
            .expect("fresh pseud");
        pseids.push((account, pseud));
    }

    let work = id("rw-work");
    exec(
        &tdb,
        "INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at) VALUES (?, ?, 'A Brigaded Work', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        "INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at) VALUES ($1::uuid, $2::uuid, 'A Brigaded Work', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        &[&work, &author_pseud],
    )
    .await;

    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    for (account, pseud) in &pseids {
        exec(
            &tdb,
            "INSERT INTO rating (id, account_id, pseud_id, work_id, stars, is_public, created_at, updated_at, version)
             VALUES (?, ?, ?, ?, 5, 1, ?, ?, 1)",
            "INSERT INTO rating (id, account_id, pseud_id, work_id, stars, is_public, created_at, updated_at, version)
             VALUES ($1::uuid, $2::uuid, $3::uuid, $4::uuid, 5, TRUE, $5, $5, 1)",
            &[
                &id(&format!("rw-rating-{pseud}")),
                account,
                pseud,
                &work,
                &now,
                &now,
            ],
        )
        .await;
    }

    // The operator, in a client built with the operator's account id in config.
    let mut operator_client = TestClient::new(build(&config, &db));
    register(&mut operator_client, "rw-op@example.test", "rwoperator").await;
    let mut operator_config = config.clone();
    operator_config.administration.operator_account_id = Some(account_of(&db, "rwoperator").await);
    let mut operator_client = TestClient::new(build(&operator_config, &db));
    test_support::sign_in_as(
        &mut operator_client,
        &tdb,
        "rw-op@example.test",
        "rwoperator",
    )
    .await;

    // 1. Detection finds the burst and records one event whose severity is the observation.
    let (status, report) = operator_client
        .post("/api/v1/admin/anomalies/detect", serde_json::json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["works_examined"], serde_json::json!(1), "{report}");
    assert_eq!(
        report["recorded"].as_array().map(Vec::len),
        Some(1),
        "{report}"
    );
    assert_eq!(report["already_flagged"], serde_json::json!(0), "{report}");
    assert_eq!(
        report["recorded"][0]["fresh_ratings"],
        serde_json::json!(5),
        "the severity is how many fresh accounts rated it: {report}"
    );

    // 2. A second run records nothing: a knighted brigade must not be knighted twice, or the severity measures how often
    //    somebody pressed the button.
    let (status, again) = operator_client
        .post("/api/v1/admin/anomalies/detect", serde_json::json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(
        again["recorded"].as_array().map(Vec::len),
        Some(0),
        "{again}"
    );
    assert_eq!(again["already_flagged"], serde_json::json!(1), "{again}");

    // 3. The queue names the work it counts. This is the assertion the old `items: []` could not pass.
    let (status, listing) = operator_client.get("/api/v1/admin/anomalies").await;
    assert_eq!(status, StatusCode::OK, "{listing}");
    assert_eq!(listing["count"], serde_json::json!(1), "{listing}");
    let items = listing["items"].as_array().expect("items");
    assert_eq!(items.len(), 1, "{listing}");
    assert_eq!(items[0]["work_id"], serde_json::json!(work), "{listing}");
    assert_eq!(items[0]["kind"], serde_json::json!("burst"), "{listing}");

    // 4. The per-work read answers the same event, with its detail parsed rather than passed through as text.
    let (status, per_work) = operator_client
        .get(format!("/api/v1/works/{work}/anomalies"))
        .await;
    assert_eq!(status, StatusCode::OK, "{per_work}");
    let events = per_work["items"].as_array().expect("items");
    assert_eq!(events.len(), 1, "{per_work}");
    assert_eq!(
        events[0]["detail"]["rule"],
        serde_json::json!("fresh_account_burst"),
        "the detail names the rule; `kind` uses the schema's vocabulary: {per_work}"
    );

    // 5. Before the mark, the public aggregate publishes a number: five public ratings clear §9.4's threshold of three.
    let (status, summary) = author_client
        .get(format!("/api/v1/works/{work}/rating-summary"))
        .await;
    assert_eq!(status, StatusCode::OK, "{summary}");
    assert_eq!(summary["mean_stars"], serde_json::json!(5.0), "{summary}");
    assert!(summary.get("suppressed").is_none(), "{summary}");

    // 6. A contested mark without a reason is refused: the reason is the only explanation anyone gets.
    let (status, body) = operator_client
        .post(
            format!("/api/v1/admin/works/{work}/contested"),
            serde_json::json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    // 7. With a reason the mark lands, and the public aggregate stops publishing a score — naming the suppression rather
    //    than answering a bare null, which would read as "nobody has rated this".
    let (status, body) = operator_client
        .post(
            format!("/api/v1/admin/works/{work}/contested"),
            serde_json::json!({ "reason": "five accounts created today rated it within the hour" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, summary) = author_client
        .get(format!("/api/v1/works/{work}/rating-summary"))
        .await;
    assert_eq!(status, StatusCode::OK, "{summary}");
    assert_eq!(
        summary["suppressed"],
        serde_json::json!("contested"),
        "{summary}"
    );
    assert!(summary["mean_stars"].is_null(), "{summary}");

    // 8. Clearing the mark publishes again.
    let contested_path = format!("/api/v1/admin/works/{work}/contested");
    let (status, body) = operator_client
        .request("DELETE", &contested_path, None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, summary) = author_client
        .get(format!("/api/v1/works/{work}/rating-summary"))
        .await;
    assert_eq!(status, StatusCode::OK, "{summary}");
    assert_eq!(summary["mean_stars"], serde_json::json!(5.0), "{summary}");

    // 9. A plain reader is 404 on every one of these doors — including the detection run, which writes rows.
    let (status, body) = author_client
        .post("/api/v1/admin/anomalies/detect", serde_json::json!({}))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, body) = author_client
        .post(
            format!("/api/v1/admin/works/{work}/contested"),
            serde_json::json!({ "reason": "not mine to say" }),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, body) = author_client.get("/api/v1/admin/anomalies").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}
