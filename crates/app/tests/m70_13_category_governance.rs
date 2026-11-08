//! Category governance (§39.2, §45.1–§45.4): the seed, the proposal queue, the ceiling (M70-13).
//!
//! Five store functions had no caller and the absence was not visible: a fresh instance had an **empty** category list, so
//! the directory's category filter offered nothing and `create_proposal` answered "category not found" for every built-in
//! slug an operator would try. A list with no rows is a valid list, and nothing logged.
//!
//! The claims:
//!
//! 1. **The §39.2 slugs are seeded at boot, idempotently** — `ON CONFLICT(slug) DO NOTHING` is §45.1's rule for a config
//!    upsert: it adds what is missing and never resurrects a category the community removed.
//! 2. **The seed is wired into the boot path**, asserted against the source the same way the route inventory is —
//!    `serve` binds a listener and no test may boot one.
//! 3. **The queue expires before it answers.** A proposal whose window closed must not sit in the queue as `open`: the
//!    binary has no scheduler, so the read is the sweep, and the count of expired rows is returned rather than implied.
//! 4. **The ceiling is visible and enforced.** `governance_state` reports active count against the ceiling, and a create
//!    proposal at the ceiling is refused at propose time — a proposal that cannot succeed wastes quorum attention.
//! 5. **`set_max_categories` refuses instead of pretending.** It used to answer 200 with the unchanged configured value,
//!    which a client could only read as "applied".

use axum::http::StatusCode;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::{category_governance as cg_db, Database};
use test_support::{register, scratch_dir, sign_in_as, TestClient, TestDb};

fn build(config: &Config, db: &Database) -> axum::Router {
    server::build_router(AppState::new(config.clone(), db.clone()))
}

#[tokio::test]
async fn the_categories_are_seeded_the_queue_expires_and_the_ceiling_is_a_door_that_means_it() {
    let dir = scratch_dir("category-governance");
    let tdb = TestDb::connect_with_dir("m7013-category-governance", &dir).await;
    let db: Database = tdb.db().clone();
    let config = Config::development_defaults();
    let state = AppState::new(config.clone(), db.clone());

    // 1. Seeding is idempotent and produces §39.2's eight slugs.
    let first = darkhold_app::directory_seed::seed_directory_categories(&state)
        .await
        .expect("seed");
    assert_eq!(first, 8, "eight built-in categories, not {first}");
    let second = darkhold_app::directory_seed::seed_directory_categories(&state)
        .await
        .expect("re-seed");
    assert_eq!(second, 8, "a second boot adds nothing: {second}");

    // A category the community removed stays removed: the seed must not resurrect it.
    cg_db::deprecate_category(&db, "podcast_newsletter")
        .await
        .expect("deprecate");
    darkhold_app::directory_seed::seed_directory_categories(&state)
        .await
        .expect("third seed");
    let row = cg_db::get_category(&db, "podcast_newsletter")
        .await
        .expect("read")
        .expect("row survives");
    assert_eq!(
        row.state, "deprecated",
        "a config upsert never resurrects a category the community removed (§45.1)"
    );

    // 2. The boot path really calls it. `serve` binds a listener, so no test may boot the real server; the wiring is
    //    asserted against the source, the same way `route_inventory` checks the route table against the routers.
    let server_source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/server.rs"
    ))
    .expect("server.rs");
    assert!(
        server_source.contains("crate::directory_seed::seed_directory_categories(&state)"),
        "the directory seed must be called from the boot path, or a fresh instance has no categories"
    );

    // 3. The queue: an open proposal inside its window stays, one past its window is expired by the read.
    let mut operator = TestClient::new(build(&config, &db));
    let operator_account = register(&mut operator, "cg-operator@example.test", "cgoperator").await;
    let mut governed = config.clone();
    governed.administration.operator_account_id = Some(operator_account.parse().expect("account id"));
    let mut client = TestClient::new(build(&governed, &db));
    sign_in_as(&mut client, &tdb, "cg-operator@example.test", "cgoperator").await;

    // The route needs CategoryGovernance privilege; grant it by trust rather than by role claim.
    darkhold_db::governance::set_trust(&db, &operator_account, 5, "test: category governance")
        .await
        .expect("trust");

    let live = cg_db::create_proposal(
        &db,
        "community",
        darkhold_domain::category_governance::ProposalAction::Rename,
        "{\"label\":\"Communities\"}",
        3,
        "2099-01-01T00:00:00Z",
        &operator_account,
        "2026-11-08T00:00:00Z",
    )
    .await
    .expect("live proposal");
    let lapsed = cg_db::create_proposal(
        &db,
        "community",
        darkhold_domain::category_governance::ProposalAction::Rename,
        "{\"label\":\"Something Else\"}",
        3,
        "2026-11-07T00:00:00Z",
        &operator_account,
        "2026-11-01T00:00:00Z",
    )
    .await
    .expect("lapsed proposal");

    let (status, queue) = client
        .get("/api/v1/directory/categories/governance/proposals?category=community")
        .await;
    assert_eq!(status, StatusCode::OK, "{queue}");
    assert_eq!(queue["count"], serde_json::json!(1), "{queue}");
    assert_eq!(queue["items"][0]["id"], serde_json::json!(live), "{queue}");
    assert_eq!(
        queue["expired"],
        serde_json::json!(1),
        "the read is the sweep: {queue}"
    );
    let lapsed_row = cg_db::get_proposal(&db, &lapsed)
        .await
        .expect("read")
        .expect("row");
    assert_eq!(
        lapsed_row.status, "expired",
        "a proposal past its TTL is expired, not still open"
    );

    // A second read finds nothing left to expire: the sweep is idempotent because the status already moved.
    let (status, again) = client
        .get("/api/v1/directory/categories/governance/proposals?category=community")
        .await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["expired"], serde_json::json!(0), "{again}");

    // An unknown category is 404 rather than an empty list, which would read as "nothing proposed".
    let (status, body) = client
        .get("/api/v1/directory/categories/governance/proposals?category=nope")
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // 4. The ceiling is visible in the state, and enforced when a create is proposed.
    let (status, state_now) = client.get("/api/v1/directory/categories/governance").await;
    assert_eq!(status, StatusCode::OK, "{state_now}");
    assert_eq!(
        state_now["active_categories"],
        serde_json::json!(7),
        "eight seeded, one deprecated: {state_now}"
    );
    assert_eq!(state_now["at_ceiling"], serde_json::json!(false), "{state_now}");

    // Push the instance to its ceiling by adding categories directly, then try to propose a create.
    let max = config.directory.governance.max_active_categories as i64;
    let now = darkhold_db::identity::now_rfc3339();
    let mut added = 0;
    while cg_db::count_active_categories(&db).await.expect("count") < max {
        cg_db::upsert_category(
            &db,
            &format!("filler-{added}"),
            &format!("Filler {added}"),
            "seed",
            "system",
            &now,
        )
        .await
        .expect("filler");
        added += 1;
    }
    let (status, body) = client
        .post(
            "/api/v1/directory/categories/governance/proposals",
            serde_json::json!({
                "category_slug": "other",
                "action": "create",
                "payload": {"slug": "one-too-many", "label": "One Too Many"},
            }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a create proposal at the ceiling must be refused at propose time: {body}"
    );
    assert!(
        body.to_string().contains("ceiling"),
        "the refusal says which ceiling: {body}"
    );

    // 5. `set_max_categories` no longer claims a change it cannot make.
    let (status, body) = client
        .post(
            "/api/v1/directory/categories/governance/max",
            serde_json::json!({"max": (max as u32) + 10}),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a runtime ceiling change that cannot persist must be refused, not echoed back: {body}"
    );
    assert!(
        body.to_string().contains("max_active_categories"),
        "the refusal names where the value lives: {body}"
    );

    // Sending the value it already has is a no-op the client can trust.
    let (status, unchanged) = client
        .post(
            "/api/v1/directory/categories/governance/max",
            serde_json::json!({"max": max as u32}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{unchanged}");
    assert_eq!(
        unchanged["max_active_categories"],
        serde_json::json!(max as u32),
        "{unchanged}"
    );
}
