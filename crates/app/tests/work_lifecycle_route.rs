//! M45-76 Step 4.2: the operator door for the lifecycle sweep.
//!
//! Default body is a dry run. A write needs both an operator session and
//! `[works].auto_abandon = true`; turning automation off must not take away the
//! ability to inspect what the sweep would do.

use axum::http::StatusCode;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use serde_json::json;
use test_support::{scratch_dir, sign_in_as, TestClient, TestDb};

const EMAIL: &str = "lifecycle-operator@example.com";
const HANDLE: &str = "lifecycleoperator";
const WORK: &str = "01957600-0000-4000-8000-00000000e003";

struct Harness {
    tdb: TestDb,
    config: Config,
}

impl Harness {
    async fn new(tag: &str) -> Self {
        let dir = scratch_dir(tag);
        let tdb = TestDb::connect_with_dir(tag, &dir).await;
        let mut config = Config::development_defaults();
        config.storage.root = dir.clone();
        let bootstrap = Self { tdb, config };
        let mut client = bootstrap.client();
        let account = sign_in_as(&mut client, &bootstrap.tdb, EMAIL, HANDLE).await;
        drop(client);
        let mut out = bootstrap;
        out.config.administration.operator_account_id =
            Some(account.parse().expect("operator account uuid"));
        out
    }

    fn client(&self) -> TestClient {
        TestClient::new(server::build_router(AppState::new(
            self.config.clone(),
            self.tdb.db().clone(),
        )))
    }
}

async fn exec(db: &darkhold_db::Database, sql: &str) {
    sqlx::query(sql)
        .execute(db.sqlite_pool().expect("sqlite handle"))
        .await
        .expect("fixture");
}

async fn seed(h: &Harness) {
    let db = h.tdb.db();
    exec(db, "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version) VALUES ('01957600-0000-4000-8000-00000000e001','active','stale@test.dev','adult','2001-01-01T00:00:00Z','2001-01-01T00:00:00Z',1)").await;
    exec(db, "INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at, updated_at, version) VALUES ('01957600-0000-4000-8000-00000000e002','01957600-0000-4000-8000-00000000e001','stale','Stale','listed','2001-01-01T00:00:00Z','2001-01-01T00:00:00Z',1)").await;
    exec(db, &format!("INSERT INTO works (id, owner_pseud_id, title, visibility, lifecycle, completion, created_at, updated_at) VALUES ('{WORK}','01957600-0000-4000-8000-00000000e002','Stale','public','published','in_progress','2001-01-01T00:00:00Z','2001-01-01T00:00:00Z')")).await;
    exec(db, "INSERT INTO library_items (id, account_id, work_id, source_key, source_work_key, title, status, source_updated_at, source_url, created_at, updated_at) VALUES ('01957600-0000-4000-8000-00000000e004','01957600-0000-4000-8000-00000000e001','01957600-0000-4000-8000-00000000e003','sweep','w1','Stale','ongoing','2001-01-01T00:00:00Z','https://example.invalid/1','2001-01-01T00:00:00Z','2001-01-01T00:00:00Z')").await;
}

async fn completion(db: &darkhold_db::Database) -> String {
    sqlx::query_scalar(&format!("SELECT completion FROM works WHERE id = '{WORK}'"))
        .fetch_one(db.sqlite_pool().expect("sqlite handle"))
        .await
        .expect("read completion")
}

#[tokio::test]
async fn the_operator_sweep_defaults_to_dry_run_and_refuses_writes_when_disabled() {
    let mut h = Harness::new("lifecycle_route").await;
    seed(&h).await;

    assert!(
        darkhold_app::routes::inventory::ROUTE_TABLE
            .iter()
            .any(|entry| { entry.method == "POST" && entry.path == "/lifecycle/sweep" }),
        "the new route must be in the inventory with its audience"
    );

    let mut operator = h.client();
    sign_in_as(&mut operator, &h.tdb, EMAIL, HANDLE).await;
    let (status, body) = operator.post("/api/v1/lifecycle/sweep", json!({})).await;
    assert_eq!(status, StatusCode::OK, "default body is a dry run: {body}");
    assert_eq!(body["dry_run"], true);
    assert_eq!(body["abandoned"], 1);
    assert_eq!(completion(h.tdb.db()).await, "in_progress");

    let (status, body) = operator
        .post(
            "/api/v1/lifecycle/sweep",
            json!({ "dry_run": false, "limit": 10 }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "an operator may apply: {body}");
    assert_eq!(body["abandoned"], 1);
    assert_eq!(completion(h.tdb.db()).await, "abandoned");

    h.config.works.auto_abandon = false;
    let mut disabled = h.client();
    sign_in_as(&mut disabled, &h.tdb, EMAIL, HANDLE).await;
    let (status, body) = disabled
        .post("/api/v1/lifecycle/sweep", json!({ "dry_run": false }))
        .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a write must be refused: {body}"
    );
    let (status, body) = disabled.post("/api/v1/lifecycle/sweep", json!({})).await;
    assert_eq!(status, StatusCode::OK, "inspection survives: {body}");
    assert_eq!(body["dry_run"], true);

    h.tdb.cleanup().await;
}
