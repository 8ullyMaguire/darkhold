//! M45-76 Step 6: the full lifecycle suite through HTTP, on either engine.
//!
//! `DARKHOLD_TEST_PG_URL` selects PostgreSQL; without it every case runs on a
//! scratch SQLite database. No test calls a store function directly: the
//! assertions are about what an operator or reviewer can actually do.

use axum::http::StatusCode;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::Backend;
use serde_json::{json, Value};
use test_support::{exec_binds, scratch_dir, sign_in_as, TestClient, TestDb};
use time::OffsetDateTime;

const OPS_EMAIL: &str = "lifecycle-ops@example.com";
const OPS_HANDLE: &str = "lifecycleops";
const OWNER_ACCOUNT: &str = "01957600-0000-4000-8000-00000000b001";
const OWNER_PSEUD: &str = "01957600-0000-4000-8000-00000000b002";
const WORK: &str = "01957600-0000-4000-8000-00000000b003";
const LIB_ITEM: &str = "01957600-0000-4000-8000-00000000b004";
const OLD: &str = "2001-01-01T00:00:00Z";

fn days_ago(days: i64) -> String {
    let value = OffsetDateTime::now_utc() - time::Duration::days(days);
    // The library trigger requires canonical `…Z`, not the `+00:00` form that
    // the generic RFC 3339 formatter emits for this crate's `time` version.
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        value.year(),
        value.month() as u8,
        value.day(),
        value.hour(),
        value.minute(),
        value.second()
    )
}

fn config_for(dir: &std::path::Path) -> Config {
    let mut config = Config::development_defaults();
    config.storage.root = dir.to_path_buf();
    config.database = match std::env::var("DARKHOLD_TEST_PG_URL") {
        Ok(url) => darkhold_db::DatabaseConfig::new(url),
        Err(_) => darkhold_db::DatabaseConfig::new(format!(
            "sqlite://{}/darkhold.sqlite?mode=rwc",
            dir.display()
        )),
    };
    let generous = darkhold_app::limiter::Quota {
        burst: 1000,
        per_minute: 6000,
    };
    config.rate_limits.auth = generous;
    config.rate_limits.write = generous;
    config.rate_limits.default = generous;
    config
}

struct Harness {
    tdb: TestDb,
    config: Config,
    operator: TestClient,
    _dir: std::path::PathBuf,
}

impl Harness {
    async fn new(tag: &str) -> Self {
        let dir = scratch_dir(tag);
        let tdb = TestDb::connect_with_dir(tag, &dir).await;
        let mut config = config_for(&dir);
        let mut first = TestClient::new(server::build_router(AppState::new(
            config.clone(),
            tdb.db().clone(),
        )));
        let account = sign_in_as(&mut first, &tdb, OPS_EMAIL, OPS_HANDLE).await;
        config.administration.operator_account_id = Some(account.parse().expect("operator uuid"));
        let mut operator = TestClient::new(server::build_router(AppState::new(
            config.clone(),
            tdb.db().clone(),
        )));
        sign_in_as(&mut operator, &tdb, OPS_EMAIL, OPS_HANDLE).await;
        Self {
            tdb,
            config,
            operator,
            _dir: dir,
        }
    }

    fn client(&self) -> TestClient {
        TestClient::new(server::build_router(AppState::new(
            self.config.clone(),
            self.tdb.db().clone(),
        )))
    }

    async fn sweep(&mut self, body: Value) -> (StatusCode, Value) {
        self.operator.post("/api/v1/lifecycle/sweep", body).await
    }
}

async fn seed(
    h: &Harness,
    work_id: &str,
    lifecycle: &str,
    completion: &str,
    marked: &str,
    source_at: &str,
    library_status: &str,
) {
    let db = h.tdb.db();
    exec_binds(
        db,
        "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version) VALUES (?, 'active', ?, 'adult', '2001-01-01T00:00:00Z', '2001-01-01T00:00:00Z', 1)",
        "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version) VALUES ($1::uuid, 'active', $2, 'adult', '2001-01-01T00:00:00Z', '2001-01-01T00:00:00Z', 1)",
        &[OWNER_ACCOUNT, "owner@example.com"],
    )
    .await;
    exec_binds(
        db,
        "INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at, updated_at, version) VALUES (?, ?, 'workowner', 'Owner', 'listed', '2001-01-01T00:00:00Z', '2001-01-01T00:00:00Z', 1)",
        "INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at, updated_at, version) VALUES ($1::uuid, $2::uuid, 'workowner', 'Owner', 'listed', '2001-01-01T00:00:00Z', '2001-01-01T00:00:00Z', 1)",
        &[OWNER_PSEUD, OWNER_ACCOUNT],
    )
    .await;
    exec_binds(
        db,
        "INSERT INTO works (id, owner_pseud_id, title, visibility, lifecycle, completion, completion_marked_at, created_at, updated_at, version) VALUES (?, ?, 'Seeded', 'public', ?, ?, NULLIF(?, ''), '2001-01-01T00:00:00Z', '2001-01-01T00:00:00Z', 1)",
        "INSERT INTO works (id, owner_pseud_id, title, visibility, lifecycle, completion, completion_marked_at, created_at, updated_at, version) VALUES ($1::uuid, $2::uuid, 'Seeded', 'public', $3, $4, NULLIF($5, ''), '2001-01-01T00:00:00Z', '2001-01-01T00:00:00Z', 1)",
        &[work_id, OWNER_PSEUD, lifecycle, completion, marked],
    )
    .await;
    exec_binds(
        db,
        "INSERT INTO work_contributors (work_id, pseud_id, role, public_attribution, created_at) VALUES (?, ?, 'owner', 1, ?)",
        "INSERT INTO work_contributors (work_id, pseud_id, role, public_attribution, created_at) VALUES ($1::uuid, $2::uuid, 'owner', 1, $3)",
        &[work_id, OWNER_PSEUD, OLD],
    )
    .await;
    exec_binds(
        db,
        "INSERT INTO library_items (id, account_id, work_id, source_key, source_work_key, title, status, source_updated_at, source_url, created_at, updated_at) VALUES (?, ?, ?, 'key', 'remote', 'Seeded', ?, ?, 'https://example.invalid/seed', ?, ?)",
        "INSERT INTO library_items (id, account_id, work_id, source_key, source_work_key, title, status, source_updated_at, source_url, created_at, updated_at) VALUES ($1::uuid, $2::uuid, $3::uuid, 'key', 'remote', 'Seeded', $4, $5, 'https://example.invalid/seed', $6, $7)",
        &[LIB_ITEM, OWNER_ACCOUNT, work_id, library_status, source_at, OLD, OLD],
    )
    .await;
}

async fn state(h: &Harness, work_id: &str) -> (String, String, Option<String>) {
    let db = h.tdb.db();
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as("SELECT completion, completion_source, completion_marked_at FROM works WHERE id = ?")
                .bind(work_id)
                .fetch_one(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("work state")
        }
        Backend::Postgres => {
            sqlx::query_as("SELECT completion, completion_source, completion_marked_at FROM works WHERE id::text = $1")
                .bind(work_id)
                .fetch_one(db.postgres_pool().expect("postgres"))
                .await
                .expect("work state")
        }
    }
}

async fn marks(h: &Harness, work_id: &str, source: &str) -> i64 {
    let db = h.tdb.db();
    match db.backend() {
        Backend::Sqlite => sqlx::query_scalar(
            "SELECT COUNT(*) FROM work_completion_marks WHERE work_id = ? AND source = ?",
        )
        .bind(work_id)
        .bind(source)
        .fetch_one(db.sqlite_pool().expect("sqlite"))
        .await
        .expect("marks"),
        Backend::Postgres => sqlx::query_scalar(
            "SELECT COUNT(*) FROM work_completion_marks WHERE work_id::text = $1 AND source = $2",
        )
        .bind(work_id)
        .bind(source)
        .fetch_one(db.postgres_pool().expect("postgres"))
        .await
        .expect("marks"),
    }
}

async fn set_trust(h: &Harness, account: &str, level: i64) {
    exec_binds(
        h.tdb.db(),
        "UPDATE trust_levels SET level = ?, basis = 'operator', computed_at = '2001-01-01T00:00:00Z' WHERE account = ?",
        &format!("UPDATE trust_levels SET level = $1::integer, basis = 'operator', computed_at = '2001-01-01T00:00:00Z' WHERE account = $2"),
        &[&level.to_string(), account],
    )
    .await;
}

async fn reviewer(h: &Harness, email: &str, handle: &str, level: i64) -> (TestClient, String) {
    let mut client = h.client();
    let account = sign_in_as(&mut client, &h.tdb, email, handle).await;
    set_trust(h, &account, level).await;
    (client, account)
}

async fn complete(client: &mut TestClient, work_id: &str, value: &str) -> (StatusCode, Value) {
    client
        .post(
            &format!("/api/v1/works/{work_id}/completion"),
            json!({ "completion": value }),
        )
        .await
}

#[tokio::test]
async fn a_stale_in_progress_work_is_abandoned_by_a_write_sweep() {
    let mut h = Harness::new("m46_sweep_abandon").await;
    seed(
        &h,
        WORK,
        "published",
        "in_progress",
        "",
        &days_ago(121),
        "ongoing",
    )
    .await;
    let (status, body) = h.sweep(json!({ "dry_run": false, "limit": 10 })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["abandoned"], 1);
    let (completion, source, _) = state(&h, WORK).await;
    assert_eq!(completion, "abandoned");
    assert_eq!(source, "sweep");
    assert_eq!(marks(&h, WORK, "sweep").await, 1);
    h.tdb.cleanup().await;
}

#[tokio::test]
async fn a_dry_run_writes_nothing() {
    let mut h = Harness::new("m46_sweep_dry").await;
    seed(
        &h,
        WORK,
        "published",
        "in_progress",
        "",
        &days_ago(121),
        "ongoing",
    )
    .await;
    let (status, body) = h.sweep(json!({ "dry_run": true, "limit": 10 })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["abandoned"], 1);
    let (completion, source, _) = state(&h, WORK).await;
    assert_eq!(completion, "in_progress");
    assert_eq!(source, "import");
    assert_eq!(marks(&h, WORK, "sweep").await, 0);
    h.tdb.cleanup().await;
}

#[tokio::test]
async fn a_complete_work_is_never_swept() {
    let mut h = Harness::new("m46_sweep_complete").await;
    seed(
        &h,
        WORK,
        "published",
        "complete",
        "",
        &days_ago(121),
        "ongoing",
    )
    .await;
    let (status, body) = h.sweep(json!({ "dry_run": false, "limit": 10 })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["abandoned"], 0);
    assert_eq!(state(&h, WORK).await.0, "complete");
    h.tdb.cleanup().await;
}

#[tokio::test]
async fn a_source_complete_work_is_concluded() {
    let mut h = Harness::new("m46_sweep_conclude").await;
    seed(
        &h,
        WORK,
        "published",
        "in_progress",
        "",
        &days_ago(121),
        "complete",
    )
    .await;
    let (status, body) = h.sweep(json!({ "dry_run": false, "limit": 10 })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["concluded"], 1);
    let (completion, source, _) = state(&h, WORK).await;
    assert_eq!(completion, "complete");
    assert_eq!(source, "update");
    h.tdb.cleanup().await;
}

#[tokio::test]
async fn a_hiatus_work_is_swept() {
    let mut h = Harness::new("m46_sweep_hiatus").await;
    seed(
        &h,
        WORK,
        "published",
        "hiatus",
        "",
        &days_ago(121),
        "ongoing",
    )
    .await;
    let (status, body) = h.sweep(json!({ "dry_run": false, "limit": 10 })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["abandoned"], 1);
    assert_eq!(state(&h, WORK).await.0, "abandoned");
    h.tdb.cleanup().await;
}

#[tokio::test]
async fn a_new_source_date_revives_an_abandoned_work() {
    let mut h = Harness::new("m46_sweep_revive").await;
    seed(
        &h,
        WORK,
        "published",
        "abandoned",
        &days_ago(10),
        &days_ago(1),
        "ongoing",
    )
    .await;
    let (status, body) = h.sweep(json!({ "dry_run": false, "limit": 10 })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["revived"], 1);
    assert_eq!(state(&h, WORK).await.0, "in_progress");
    h.tdb.cleanup().await;
}

#[tokio::test]
async fn a_metadata_edit_does_not_revive() {
    let mut h = Harness::new("m46_sweep_no_revive").await;
    seed(
        &h,
        WORK,
        "published",
        "abandoned",
        &days_ago(10),
        &days_ago(121),
        "ongoing",
    )
    .await;
    let (status, body) = h.sweep(json!({ "dry_run": false, "limit": 10 })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["revived"], 0);
    assert_eq!(state(&h, WORK).await.0, "abandoned");
    h.tdb.cleanup().await;
}

#[tokio::test]
async fn tl3_may_complete_a_work_it_does_not_own() {
    let h = Harness::new("m46_review_tl3").await;
    seed(
        &h,
        WORK,
        "published",
        "in_progress",
        "",
        &days_ago(1),
        "ongoing",
    )
    .await;
    let (mut client, _) = reviewer(&h, "tl3@example.com", "tl3reviewer", 3).await;
    let (status, body) = complete(&mut client, WORK, "complete").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(state(&h, WORK).await.1, "review");
    h.tdb.cleanup().await;
}

#[tokio::test]
async fn tl2_is_refused_with_the_bar_named() {
    let h = Harness::new("m46_review_tl2").await;
    seed(
        &h,
        WORK,
        "published",
        "in_progress",
        "",
        &days_ago(1),
        "ongoing",
    )
    .await;
    let (mut client, _) = reviewer(&h, "tl2@example.com", "tl2reviewer", 2).await;
    let (status, body) = complete(&mut client, WORK, "complete").await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let message = body["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("trust level 3"),
        "the refusal must name the bar, not hide it in a request id: {body}"
    );
    h.tdb.cleanup().await;
}

#[tokio::test]
async fn an_explicit_curator_at_tl0_may_complete() {
    let h = Harness::new("m46_review_curator").await;
    seed(
        &h,
        WORK,
        "published",
        "in_progress",
        "",
        &days_ago(1),
        "ongoing",
    )
    .await;
    let (mut client, account) = reviewer(&h, "curator@example.com", "curatorzero", 0).await;
    exec_binds(
        h.tdb.db(),
        "UPDATE accounts SET is_curator = 1 WHERE id = ?",
        "UPDATE accounts SET is_curator = true WHERE id::uuid = $1::uuid",
        &[&account],
    )
    .await;
    let (status, body) = complete(&mut client, WORK, "complete").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(state(&h, WORK).await.1, "review");
    h.tdb.cleanup().await;
}

#[tokio::test]
async fn a_draft_is_refused() {
    let h = Harness::new("m46_review_draft").await;
    seed(
        &h,
        WORK,
        "draft",
        "in_progress",
        "",
        &days_ago(1),
        "ongoing",
    )
    .await;
    let (mut client, _) = reviewer(&h, "draftreview@example.com", "draftreviewer", 3).await;
    let (status, body) = complete(&mut client, WORK, "complete").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    h.tdb.cleanup().await;
}

#[tokio::test]
async fn a_reviewer_cannot_set_abandoned() {
    let h = Harness::new("m46_review_abandon").await;
    seed(
        &h,
        WORK,
        "published",
        "in_progress",
        "",
        &days_ago(1),
        "ongoing",
    )
    .await;
    let (mut client, _) = reviewer(&h, "abandonreview@example.com", "abandonreviewer", 3).await;
    let (status, body) = complete(&mut client, WORK, "abandoned").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    h.tdb.cleanup().await;
}

#[tokio::test]
async fn the_second_identical_completion_is_idempotent() {
    let h = Harness::new("m46_review_idempotent").await;
    seed(
        &h,
        WORK,
        "published",
        "in_progress",
        "",
        &days_ago(1),
        "ongoing",
    )
    .await;
    let (mut client, _) = reviewer(&h, "idem@example.com", "idemreviewer", 3).await;
    let (status, body) = complete(&mut client, WORK, "complete").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = complete(&mut client, WORK, "complete").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["changed"], false);
    assert_eq!(marks(&h, WORK, "review").await, 1);
    h.tdb.cleanup().await;
}
