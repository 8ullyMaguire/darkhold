//! `GET /arena/next` over HTTP (spec §0.4.2a).
//!
//! `crates/app/tests/arena.rs` covers the STORE: the pool query, the ballot
//! recording, the weight updates. Nothing in the workspace issued the HTTP call
//! before this file — `grep -rn 'arena/next' crates --include='*.rs'` matched only
//! the route definition and the `ROUTE_TABLE` row — and no Playwright spec visited
//! `/arena` either. So the one thing that was untested was the thing a reader meets
//! first: what the route does when there is no round to give.
//!
//! It answered **500 `INTERNAL`**, and `/arena` opened with a red "That did not
//! work" for every new account — on a blank database *and* on a fully seeded one,
//! because `generate_arena_round` needs four published public works that share a
//! fandom and a fresh account has rated nothing. An empty round is an ordinary
//! state for this reader, not a server fault, so it is `200` with an explanation.
//!
//! The rule this file enforces is the one from `100-ideas-remaining.md` §3 —
//! `{"round": null}`, never a failure code — and the two cases are kept apart
//! because they are different facts a reader can act on: an archive with nothing
//! published, and an archive whose works do not yet group into fours.

use axum::http::StatusCode;

use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;

use test_support::{scratch_dir, TestClient, TestDb};

fn router_for(tdb: &TestDb, dir: &std::path::Path) -> axum::Router {
    let mut config = Config::development_defaults();
    config.storage.root = dir.to_path_buf();
    // `arena` is registered `default_on: false, toggle: per_user` — opt-in by design, so that an
    // archive that has not decided to run a calibration surface does not serve one. This file
    // tests the surface, so the instance it builds runs it: without this line every request here
    // answers `404 MODULE_UNAVAILABLE` and the four tests below assert against a module the
    // instance never mounted. That is what they did from the day the gate landed until
    // 2026-11-02, and the failure read as "the arena route is broken".
    config.modules.enabled.push("arena".to_string());
    // Pairing is deterministic here (M69-11): with exploration off, a round's mode is the decision rather than a coin,
    // so the assertion below is an assertion and not a 15%-flaky bet. The explore branch is proved by the domain's own
    // tests, which pass the roll in.
    config.discovery.arena_epsilon = 0.0;
    server::build_router(AppState::new(config, tdb.db().clone()))
}

async fn arena_reader(tdb: &TestDb, dir: &std::path::Path, tag: &str) -> TestClient {
    let mut client = TestClient::new(router_for(tdb, dir));
    let handle = tag.replace(['.', '-'], "_");
    test_support::register(&mut client, &format!("{tag}@test.dev"), &handle).await;
    client
}

/// A brand-new account on an archive with nothing published gets an empty round and
/// an explanation — not a 500.
#[tokio::test]
async fn an_empty_pool_is_an_explained_empty_round_not_a_500() {
    let dir = scratch_dir("arena_empty_pool");
    let tdb = TestDb::connect_with_dir("arena-empty-pool", &dir).await;
    let mut client = arena_reader(&tdb, &dir, "arena_empty_pool").await;

    let (status, body) = client.get("/api/v1/arena/next").await;

    assert_eq!(
        status,
        StatusCode::OK,
        "an account with nothing to compare is an ordinary state, not a server \
         fault: {body}"
    );
    assert_eq!(
        body["round"],
        serde_json::Value::Null,
        "no round is `null`, never a missing key and never an error code: {body}"
    );
    let why = body["explained_empty"]
        .as_str()
        .expect("an empty round must say why it is empty");
    assert!(
        !why.trim().is_empty(),
        "the explanation must say something: {body}"
    );
    assert!(
        why.contains("published"),
        "the empty-archive case should name its own cause, not the too-small one: {body}"
    );
    // The dimensions are what the page shows about calibration progress, so an
    // absent round must not cost the reader those too.
    assert!(
        body["dimensions"].is_array(),
        "an empty round still reports the reader's dimensions: {body}"
    );
}

/// A pool that exists but cannot form a round is a DIFFERENT answer, and says so.
///
/// The sentence differs because the reader's next action differs: an empty archive
/// is the instance operator's problem, a pool of two is theirs. Reporting both with
/// one string is the concierge's §54.6 defect in a different costume.
#[tokio::test]
async fn a_pool_too_small_to_round_names_that_specifically() {
    let dir = scratch_dir("arena_small_pool");
    let tdb = TestDb::connect_with_dir("arena-small-pool", &dir).await;
    let mut client = arena_reader(&tdb, &dir, "arena_small_pool").await;
    // Two works, not zero — enough that "nothing published" is no longer the
    // truth, and not enough for a four-card round.
    seed_published_works(&tdb, 2, "fandom_a").await;

    let (status, body) = client.get("/api/v1/arena/next").await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["round"], serde_json::Value::Null, "{body}");
    let why = body["explained_empty"].as_str().expect("an explanation");
    assert!(
        why.contains("comparable") && why.contains("fandom"),
        "a pool that exists but cannot round names the round's own requirement: {body}"
    );
    assert!(
        !why.contains("No published, public works are available"),
        "and is not reported as an empty archive, which it is not: {body}"
    );
}

/// Four published public works sharing a fandom DO produce a round, so the empty
/// answer above is not this route always declining.
#[tokio::test]
async fn four_comparable_works_still_produce_a_round() {
    let dir = scratch_dir("arena_real_round");
    let tdb = TestDb::connect_with_dir("arena-real-round", &dir).await;
    let mut client = arena_reader(&tdb, &dir, "arena_real_round").await;
    seed_published_works(&tdb, 4, "fandom_a").await;

    let (status, body) = client.get("/api/v1/arena/next").await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let round = &body["round"];
    assert!(
        !round.is_null(),
        "four published public works in one fandom are a round, so the empty \
         branch must not swallow the non-empty one: {body}"
    );
    assert_eq!(
        round["cards"].as_array().expect("cards").len(),
        4,
        "an arena round is four cards: {body}"
    );
    assert!(
        body["explained_empty"].is_null(),
        "and a round present means no explanation: {body}"
    );
}

/// The door still requires a session — this is not a fix that opened it up.
#[tokio::test]
async fn the_arena_still_needs_a_session() {
    let dir = scratch_dir("arena_unauth");
    let tdb = TestDb::connect_with_dir("arena-unauth", &dir).await;
    let mut anonymous = TestClient::new(router_for(&tdb, &dir));

    let (status, _) = anonymous.get("/api/v1/arena/next").await;

    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "§0.4.2a's arena is a signed-in reader's own calibration surface"
    );
}

/// `count` published, public, unlisted-deleted works owned by one pseud, all tagged
/// with the same fandom — the shape `get_arena_pool` selects.
///
/// Written out per engine because the id columns are TEXT on SQLite and UUID on
/// PostgreSQL, and a `?` reaching PostgreSQL is a syntax error at plan time.
async fn seed_published_works(tdb: &TestDb, count: usize, fandom: &str) -> Vec<String> {
    const T0: &str = "2026-01-01T00:00:00Z";
    let account = uuid::Uuid::new_v4().to_string();
    let pseud = uuid::Uuid::new_v4().to_string();
    let node = uuid::Uuid::new_v4().to_string();
    let owner_email = format!("owner-{account}@example.com");
    let handle = format!("owner{}", &account[..8]);

    match tdb.db().backend() {
        darkhold_db::Backend::Postgres => {
            let pool = tdb.db().postgres_pool().expect("postgres pool");
            sqlx::query(
                "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, permission_statement)
                 VALUES ($1::uuid, 'active', $2, 'adult', $3::timestamptz, $3::timestamptz, 'minimal')",
            )
            .bind(uuid::Uuid::parse_str(&account).expect("uuid"))
            .bind(&owner_email)
            .bind(T0)
            .execute(pool)
            .await
            .expect("seed account");
            sqlx::query(
                "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at)
                 VALUES ($1::uuid, $2::uuid, $3, $3, $4::timestamptz, $4::timestamptz)",
            )
            .bind(uuid::Uuid::parse_str(&pseud).expect("uuid"))
            .bind(uuid::Uuid::parse_str(&account).expect("uuid"))
            .bind(&handle)
            .bind(T0)
            .execute(pool)
            .await
            .expect("seed pseud");
            // The fandom node, and one tag per work pointing at it, so
            // `get_arena_pool`'s `canonical_entities`/`work_tags` subquery finds it.
            sqlx::query(
                "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at, review_status, signal_count)
                 VALUES ($1::uuid, 'fandom', $2, $3, $4::timestamptz, 'confirmed', 0)",
            )
            .bind(uuid::Uuid::parse_str(&node).expect("uuid"))
            .bind(fandom)
            .bind(fandom.to_lowercase())
            .bind(T0)
            .execute(pool)
            .await
            .expect("seed fandom node");
        }
        darkhold_db::Backend::Sqlite => {
            let pool = tdb.db().sqlite_pool().expect("sqlite pool");
            sqlx::query(
                "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, permission_statement)
                 VALUES (?, 'active', ?, 'adult', ?, ?, 'minimal')",
            )
            .bind(&account)
            .bind(&owner_email)
            .bind(T0)
            .bind(T0)
            .execute(pool)
            .await
            .expect("seed account");
            sqlx::query(
                "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(&pseud)
            .bind(&account)
            .bind(&handle)
            .bind(&handle)
            .bind(T0)
            .bind(T0)
            .execute(pool)
            .await
            .expect("seed pseud");
            sqlx::query(
                "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at, review_status, signal_count)
                 VALUES (?, 'fandom', ?, ?, ?, 'confirmed', 0)",
            )
            .bind(&node)
            .bind(fandom)
            .bind(fandom.to_lowercase())
            .bind(T0)
            .execute(pool)
            .await
            .expect("seed fandom node");
        }
    }

    let mut seeded = Vec::with_capacity(count);
    for n in 0..count {
        let work = uuid::Uuid::new_v4().to_string();
        seeded.push(work.clone());
        match tdb.db().backend() {
            darkhold_db::Backend::Postgres => {
                let pool = tdb.db().postgres_pool().expect("postgres pool");
                sqlx::query(
                    "INSERT INTO works (id, owner_pseud_id, title, summary, lifecycle, visibility, completion, created_at, updated_at, generated_content_posture)
                     VALUES ($1::uuid, $2::uuid, $3, $4, 'published', 'public', 'complete', $5::timestamptz, $5::timestamptz, 'forbid')",
                )
                .bind(uuid::Uuid::parse_str(&work).expect("uuid"))
                .bind(uuid::Uuid::parse_str(&pseud).expect("uuid"))
                .bind(format!("Arena work {n}"))
                .bind(format!("Summary for arena work {n}."))
                .bind(T0)
                .execute(pool)
                .await
                .expect("seed work");
                sqlx::query(
                    "INSERT INTO work_tags (work_id, node_id, prominence, added_at)
                     VALUES ($1::uuid, $2::uuid, 1, $3::timestamptz)",
                )
                .bind(uuid::Uuid::parse_str(&work).expect("uuid"))
                .bind(uuid::Uuid::parse_str(&node).expect("uuid"))
                .bind(T0)
                .execute(pool)
                .await
                .expect("tag work with the fandom");
            }
            darkhold_db::Backend::Sqlite => {
                let pool = tdb.db().sqlite_pool().expect("sqlite pool");
                sqlx::query(
                    "INSERT INTO works (id, owner_pseud_id, title, summary, lifecycle, visibility, completion, created_at, updated_at, generated_content_posture)
                     VALUES (?, ?, ?, ?, 'published', 'public', 'complete', ?, ?, 'forbid')",
                )
                .bind(&work)
                .bind(&pseud)
                .bind(format!("Arena work {n}"))
                .bind(format!("Summary for arena work {n}."))
                .bind(T0)
                .bind(T0)
                .execute(pool)
                .await
                .expect("seed work");
                sqlx::query("INSERT INTO work_tags (work_id, node_id, prominence, added_at) VALUES (?, ?, 1, ?)")
                    .bind(&work)
                    .bind(&node)
                    .bind(T0)
                    .execute(pool)
                    .await
                    .expect("tag work with the fandom");
            }
        }
    }
    seeded
}

// --- M69-11: how the four were chosen, and whether a ballot counts as calibration -----------------------------------

/// A round says which side of the pairing decision it fell on, and which cards are still being positioned.
///
/// With no taste service reachable in the harness, every card is unmeasured — which is exactly the state that must be
/// reported honestly: `proximate` over the starting value, every card calibrating, and a reason that says so. A round
/// that silently claimed settled ratings would be the failure this row exists to prevent.
#[tokio::test]
async fn a_round_reports_its_pairing_mode_and_which_cards_are_calibrating() {
    let dir = scratch_dir("arena_pairing");
    let tdb = TestDb::connect_with_dir("arena-pairing", &dir).await;
    let mut client = arena_reader(&tdb, &dir, "arena_pairing").await;
    seed_published_works(&tdb, 4, "fandom_pairing").await;

    let (status, body) = client.get("/api/v1/arena/next").await;
    assert_eq!(status, StatusCode::OK, "{body}");

    assert_eq!(
        body["mode"], "proximate",
        "with exploration configured off, every round is a proximate one: {body}"
    );
    let calibrating = body["calibrating"].as_array().expect("calibrating list");
    assert_eq!(
        calibrating.len(),
        4,
        "every card here has zero readings, and zero readings is inside the window: {body}"
    );
    let reason = body["reason"].as_str().expect("reason");
    assert!(
        reason.contains("calibrating"),
        "the round says why the label is there: {reason}"
    );
}

/// The ballot keeps the fact: a vote cast on calibrating works is stored as one.
///
/// This is what makes the window "recorded, not applied" true rather than aspirational — a consumer that ranks by the
/// ladder can leave the vote out without re-deriving which ballots were early, which it could not do anyway once more
/// ballots arrive.
#[tokio::test]
async fn a_ballot_records_that_it_compared_calibrating_works() {
    let dir = scratch_dir("arena_ballot_flag");
    let tdb = TestDb::connect_with_dir("arena-ballot-flag", &dir).await;
    let mut client = arena_reader(&tdb, &dir, "arena_ballot_flag").await;
    seed_published_works(&tdb, 4, "fandom_flag").await;

    let (status, body) = client.get("/api/v1/arena/next").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let cards: Vec<String> = body["round"]["cards"]
        .as_array()
        .expect("cards")
        .iter()
        .map(|card| card["work_id"].as_str().expect("id").to_owned())
        .collect();
    assert_eq!(cards.len(), 4, "{body}");

    let (status, body) = client
        .post(
            "/api/v1/arena/vote",
            serde_json::json!({
                "best_work_id": cards[0],
                "worst_work_id": cards[3],
                "reason_tags": [],
                "card_ids": cards,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let flagged = match tdb.db().backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar::<_, i64>(
            "SELECT calibrating FROM arena_ballots ORDER BY created_at DESC LIMIT 1",
        )
        .fetch_one(tdb.db().sqlite_pool().expect("sqlite"))
        .await
        .expect("read the ballot")
            == 1,
        // BOOLEAN on this engine: the flag is the same fact, spelled the way the dialect declares it.
        darkhold_db::Backend::Postgres => sqlx::query_scalar::<_, bool>(
            "SELECT calibrating FROM arena_ballots ORDER BY created_at DESC LIMIT 1",
        )
        .fetch_one(tdb.db().postgres_pool().expect("postgres"))
        .await
        .expect("read the ballot"),
    };
    assert!(flagged, "a ballot on works with no readings is a calibration ballot");
}

/// A work that has been compared enough stops being flagged, and the count comes from the ballots rather than from a
/// service that may not have answered.
#[tokio::test]
async fn a_work_past_its_calibration_window_is_no_longer_flagged() {
    let dir = scratch_dir("arena_calibrated");
    let tdb = TestDb::connect_with_dir("arena-calibrated", &dir).await;
    let mut client = arena_reader(&tdb, &dir, "arena_calibrated").await;
    let works = seed_published_works(&tdb, 4, "fandom_calibrated").await;

    // Five ballots from another account, all naming the first work, take it out of the window.
    let other = uuid::Uuid::new_v4().to_string();
    let now = "2026-11-07T00:00:00Z";
    match tdb.db().backend() {
        darkhold_db::Backend::Sqlite => {
            let pool = tdb.db().sqlite_pool().expect("sqlite");
            sqlx::query("INSERT INTO accounts (id, email, created_at, updated_at) VALUES (?, ?, ?, ?)")
                .bind(&other)
                .bind(format!("other-{other}@example.test"))
                .bind(now)
                .bind(now)
                .execute(pool)
                .await
                .expect("insert account");
            for _ in 0..5 {
                sqlx::query(
                    "INSERT INTO arena_ballots
                        (id, account_id, best_work_id, worst_work_id, reason_tags, created_at, calibrating)
                     VALUES (?, ?, ?, ?, '[]', ?, 1)",
                )
                .bind(uuid::Uuid::new_v4().to_string())
                .bind(&other)
                .bind(&works[0])
                .bind(&works[1])
                .bind(now)
                .execute(pool)
                .await
                .expect("insert ballot");
            }
        }
        darkhold_db::Backend::Postgres => {
            let pool = tdb.db().postgres_pool().expect("postgres");
            sqlx::query(
                "INSERT INTO accounts (id, email, created_at, updated_at) VALUES ($1::uuid, $2, $3::timestamptz, $3::timestamptz)",
            )
            .bind(&other)
            .bind(format!("other-{other}@example.test"))
            .bind(now)
            .execute(pool)
            .await
            .expect("insert account");
            for _ in 0..5 {
                sqlx::query(
                    "INSERT INTO arena_ballots
                        (id, account_id, best_work_id, worst_work_id, reason_tags, created_at, calibrating)
                     VALUES ($1::uuid, $2::uuid, $3::uuid, $4::uuid, '[]'::jsonb, $5::timestamptz, TRUE)",
                )
                .bind(uuid::Uuid::new_v4().to_string())
                .bind(&other)
                .bind(&works[0])
                .bind(&works[1])
                .bind(now)
                .execute(pool)
                .await
                .expect("insert ballot");
            }
        }
    }

    let (status, body) = client.get("/api/v1/arena/next").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let calibrating: Vec<&str> = body["calibrating"]
        .as_array()
        .expect("calibrating")
        .iter()
        .filter_map(|id| id.as_str())
        .collect();
    for past in [&works[0], &works[1]] {
        assert!(
            !calibrating.contains(&past.as_str()),
            "five comparisons is the window, so this card is no longer calibrating: {body}"
        );
    }
    assert_eq!(
        calibrating.len(),
        2,
        "the five ballots named two works (best and worst), so those two are past the window and the other two \
         are not: {body}"
    );
}
