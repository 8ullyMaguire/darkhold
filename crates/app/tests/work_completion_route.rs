//! M45-76 Step 5.1: reviewers set completion on a work they do not own.
//!
//! Authority comes from earned trust or the explicit curator grant. The route
//! writes provenance through the conditional transition, and idempotent repeats
//! answer the stored state rather than inventing a second mark.

use axum::http::StatusCode;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use serde_json::json;
use test_support::{scratch_dir, sign_in_as, TestClient, TestDb};

const PUBLISHED: &str = "01957600-0000-4000-8000-00000000f003";
const DRAFT: &str = "01957600-0000-4000-8000-00000000f004";

struct Harness {
    tdb: TestDb,
    config: Config,
}

impl Harness {
    async fn new(tag: &str) -> Self {
        let dir = scratch_dir(tag);
        let tdb = TestDb::connect_with_dir(tag, &dir).await;
        let mut config = Config::development_defaults();
        config.storage.root = dir;
        Self { tdb, config }
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
    exec(db, "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version) VALUES ('01957600-0000-4000-8000-00000000f001','active','owner@test.dev','adult','2001-01-01T00:00:00Z','2001-01-01T00:00:00Z',1)").await;
    exec(db, "INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at, updated_at, version) VALUES ('01957600-0000-4000-8000-00000000f002','01957600-0000-4000-8000-00000000f001','owner','Owner','listed','2001-01-01T00:00:00Z','2001-01-01T00:00:00Z',1)").await;
    exec(db, &format!("INSERT INTO works (id, owner_pseud_id, title, visibility, lifecycle, completion, created_at, updated_at) VALUES ('{PUBLISHED}','01957600-0000-4000-8000-00000000f002','Published','public','published','in_progress','2001-01-01T00:00:00Z','2001-01-01T00:00:00Z')")).await;
    exec(db, &format!("INSERT INTO works (id, owner_pseud_id, title, visibility, lifecycle, completion, created_at, updated_at) VALUES ('{DRAFT}','01957600-0000-4000-8000-00000000f002','Draft','private','draft','in_progress','2001-01-01T00:00:00Z','2001-01-01T00:00:00Z')")).await;
}

async fn trust(db: &darkhold_db::Database, account: &str, level: i64) {
    exec(
        db,
        &format!(
            "UPDATE trust_levels SET level = {level}, computed_at = '2001-01-01T00:00:00Z', basis = 'operator' WHERE account = '{account}'"
        ),
    )
    .await;
}

async fn sign_in(h: &Harness, email: &str, handle: &str) -> (TestClient, String) {
    let mut client = h.client();
    let account = sign_in_as(&mut client, &h.tdb, email, handle).await;
    (client, account)
}

#[tokio::test]
async fn earned_trust_and_explicit_curators_may_review_but_tl1_may_not() {
    let h = Harness::new("completion_review").await;
    seed(&h).await;
    assert!(
        darkhold_app::routes::inventory::ROUTE_TABLE
            .iter()
            .any(|entry| { entry.method == "POST" && entry.path == "/works/{id}/completion" }),
        "the completion route must be in the inventory"
    );

    let (mut tl3, tl3_id) = sign_in(&h, "reviewer-tl3@example.com", "reviewerthree").await;
    trust(h.tdb.db(), &tl3_id, 3).await;
    let (status, body) = tl3
        .post(
            &format!("/api/v1/works/{PUBLISHED}/completion"),
            json!({ "completion": "complete", "note": "source finished" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "TL3 may review: {body}");
    assert_eq!(body["changed"], true);
    let source: String = sqlx::query_scalar(
        "SELECT completion_source FROM works WHERE id = '01957600-0000-4000-8000-00000000f003'",
    )
    .fetch_one(h.tdb.db().sqlite_pool().expect("sqlite handle"))
    .await
    .expect("read provenance");
    assert_eq!(source, "review");

    let (mut tl1, tl1_id) = sign_in(&h, "reviewer-tl1@example.com", "reviewerone").await;
    trust(h.tdb.db(), &tl1_id, 1).await;
    let (status, _) = tl1
        .post(
            &format!("/api/v1/works/{PUBLISHED}/completion"),
            json!({ "completion": "in_progress" }),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (mut curator, curator_id) = sign_in(&h, "curator@example.com", "curatorreview").await;
    exec(
        h.tdb.db(),
        &format!("UPDATE accounts SET is_curator = 1 WHERE id = '{curator_id}'"),
    )
    .await;
    let (status, body) = curator
        .post(
            &format!("/api/v1/works/{PUBLISHED}/completion"),
            json!({ "completion": "in_progress" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "curator may review: {body}");
    assert_eq!(body["changed"], true);
    let (status, body) = curator
        .post(
            &format!("/api/v1/works/{PUBLISHED}/completion"),
            json!({ "completion": "in_progress" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "repeat is not a write: {body}");
    assert_eq!(body["changed"], false);

    let (status, body) = curator
        .post(
            &format!("/api/v1/works/{DRAFT}/completion"),
            json!({ "completion": "complete" }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "draft refused: {body}"
    );
    let (status, body) = curator
        .post(
            &format!("/api/v1/works/{PUBLISHED}/completion"),
            json!({ "completion": "abandoned" }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "reviewer cannot abandon: {body}"
    );

    h.tdb.cleanup().await;
}
