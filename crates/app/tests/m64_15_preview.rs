//! M64-15: an operator can preview a chosen reader's feed without writing.
//!
//! The preview is a read. If it assigns an experiment arm, records impressions
//! or builds shadow samples, then the operator's inspection changes the thing
//! being inspected and the console cannot be trusted. The mutant for this row
//! removes the `log_impressions` guard on arm assignment, so the no-assignment
//! assertion below is the one that has to redden.

use axum::http::StatusCode;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::Backend;
use darkhold_db::DatabaseConfig;
use serde_json::{json, Value};
use test_support::{sign_in_as, TestClient, TestDb};

const PASSWORD: &str = "a-long-enough-passphrase";

fn scratch(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("darkhold-m64-15-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn config_for(dir: &std::path::Path) -> Config {
    let mut config = Config::development_defaults();
    config.storage.root = dir.to_path_buf();
    config.database = match std::env::var("DARKHOLD_TEST_PG_URL") {
        Ok(url) => DatabaseConfig::new(url),
        Err(_) => DatabaseConfig::new(format!(
            "sqlite://{}/darkhold.sqlite?mode=rwc",
            dir.display()
        )),
    };
    // Two buckets make a write from `experiment_arm` visible. With one bucket
    // the handler would still call `bucket_for`, but the test could not tell.
    config.discovery.experiment_buckets = 2;
    for quota in [
        &mut config.rate_limits.auth,
        &mut config.rate_limits.write,
        &mut config.rate_limits.default,
    ] {
        *quota = darkhold_app::limiter::Quota {
            burst: 1000,
            per_minute: 6000,
        };
    }
    config
}

struct Harness {
    tdb: TestDb,
    config: Config,
}

impl Harness {
    async fn new(tag: &str, operator_handle: &str) -> Self {
        let dir = scratch(tag);
        let mut config = config_for(&dir);
        let tdb = TestDb::connect_with_dir(tag, &dir).await;
        let mut bootstrap = TestClient::new(server::build_router(AppState::new(
            config.clone(),
            tdb.db().clone(),
        )));
        let (status, body) = bootstrap
            .post(
                "/api/v1/auth/register",
                json!({
                    "email": format!("{operator_handle}@example.com"),
                    "password": PASSWORD,
                    "handle": operator_handle,
                    "display_name": operator_handle,
                    "age_band": "adult",
                }),
            )
            .await;
        assert!(status.is_success(), "register operator: {status} {body}");
        let pseud = darkhold_db::identity::find_pseud_by_handle(tdb.db(), operator_handle)
            .await
            .expect("lookup")
            .expect("operator pseud");
        config.administration.operator_account_id = Some(pseud.account_id);
        Self { tdb, config }
    }

    fn client(&self) -> TestClient {
        TestClient::new(server::build_router(AppState::new(
            self.config.clone(),
            self.tdb.db().clone(),
        )))
    }
}

async fn make_reader(h: &Harness, handle: &str) -> TestClient {
    let mut client = h.client();
    let account = sign_in_as(
        &mut client,
        &h.tdb,
        &format!("{handle}@example.com"),
        handle,
    )
    .await;
    assert!(!account.is_empty());
    client
}

#[tokio::test]
async fn operator_preview_is_a_read() {
    let h = Harness::new("operator-read", "m6415-op").await;
    let _reader = make_reader(&h, "m6415-reader").await;
    let mut operator = h.client();
    let account = sign_in_as(&mut operator, &h.tdb, "m6415-op@example.com", "m6415-op").await;
    assert!(!account.is_empty());

    let pseud = darkhold_db::identity::find_pseud_by_handle(h.tdb.db(), "m6415-reader")
        .await
        .expect("lookup")
        .expect("reader pseud");

    let (status, body) = operator
        .post(
            "/api/v1/operator/experiments/discover.feed/preview",
            json!({ "pseud": "m6415-reader" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "preview body: {body}");
    assert_eq!(body["surface"], "discover.feed");
    assert_eq!(body["persona"], "m6415-reader");
    assert!(body["feed"]["items"].is_array(), "body: {body}");

    let sql = h.tdb.db().sql(
        "SELECT COUNT(*) FROM experiment_assignments WHERE pseud_id = ?",
        "SELECT COUNT(*) FROM experiment_assignments WHERE pseud_id = $1::uuid",
    );
    let id = pseud.id.to_string();
    let assignments: i64 = match h.tdb.db().backend() {
        Backend::Sqlite => sqlx::query_scalar(&sql)
            .bind(&id)
            .fetch_one(h.tdb.db().sqlite_pool().expect("sqlite"))
            .await
            .expect("count assignments"),
        Backend::Postgres => sqlx::query_scalar(&sql)
            .bind(&id)
            .fetch_one(h.tdb.db().postgres_pool().expect("postgres"))
            .await
            .expect("count assignments"),
    };
    assert_eq!(
        assignments, 0,
        "a preview must not assign an experiment arm"
    );

    let slot_sql = h.tdb.db().sql(
        "SELECT COUNT(*) FROM recommendation_slots WHERE pseud_id = ?",
        "SELECT COUNT(*) FROM recommendation_slots WHERE pseud_id = $1::uuid",
    );
    let slots: i64 = match h.tdb.db().backend() {
        Backend::Sqlite => sqlx::query_scalar(&slot_sql)
            .bind(&id)
            .fetch_one(h.tdb.db().sqlite_pool().expect("sqlite"))
            .await
            .expect("count slots"),
        Backend::Postgres => sqlx::query_scalar(&slot_sql)
            .bind(&id)
            .fetch_one(h.tdb.db().postgres_pool().expect("postgres"))
            .await
            .expect("count slots"),
    };
    assert_eq!(slots, 0, "a preview must not record impression slots");
}

#[tokio::test]
async fn preview_rejects_an_unknown_surface() {
    let h = Harness::new("operator-surface", "m6415-op2").await;
    let mut operator = h.client();
    let _ = sign_in_as(&mut operator, &h.tdb, "m6415-op2@example.com", "m6415-op2").await;
    let (status, body): (StatusCode, Value) = operator
        .post(
            "/api/v1/operator/experiments/not.a.surface/preview",
            json!({ "pseud": "m6415-op2" }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["error"]["code"], "VALIDATION_FAILED", "{body}");
}

#[tokio::test]
async fn non_operator_cannot_preview() {
    let h = Harness::new("operator-gate", "m6415-op3").await;
    let mut reader = make_reader(&h, "m6415-reader3").await;
    let (status, body) = reader
        .post(
            "/api/v1/operator/experiments/discover.feed/preview",
            json!({ "pseud": "m6415-reader3" }),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}
