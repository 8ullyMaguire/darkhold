//! §53.5 / `M66-09` — the hit rate gets an HTTP door.
//!
//! The metric shipped as `darkhold_db::hit_rate` and **nothing served it**, so the operator's one
//! number could only be read by writing SQL against the instance by hand. These tests are about the
//! door and its three refusals, not about the arithmetic — `crates/db/tests/hit_rate.rs` owns that.
//!
//! Four things this file pins, each because the plausible alternative is wrong in a way that looks
//! like a feature:
//!
//! 1. **The operator's own number, over the window it asked for** — `shown`, `hits`, `rate`.
//! 2. **An empty window is `null`, not `0.0`**, and says so in a field (`defined`), because a zero
//!    reads as total failure and invites a change to a recipe nothing has tested yet.
//! 3. **A non-operator gets `404`**, following `flows`, `admin_discovery` and `decision_service`: for
//!    an operator surface the existence is the disclosure.
//! 4. **A disabled door answers exactly like a refused one.** Two codes would let a caller map an
//!    operator's configuration from outside.
//!
//! Runs on SQLite, and on PostgreSQL when `DARKHOLD_TEST_PG_URL` is set — the seeding helper carries
//! both dialects because `pseud_id`/`work_id` are `TEXT` on one and `uuid` on the other.

use axum::http::StatusCode;
use serde_json::json;

use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;

use test_support::{scratch_dir, sign_in_as, TestClient, TestDb, TEST_PASSWORD};

const DOOR: &str = "/api/v1/operator/hit-rate";

struct Harness {
    tdb: TestDb,
    config: Config,
    db: darkhold_db::Database,
    /// The operator's handle — the fixture looks its pseud up by handle, so it has to carry the one
    /// this harness registered rather than a constant (the first version hard-coded it and every
    /// test but the first failed on `RowNotFound`).
    handle: String,
    _dir: std::path::PathBuf,
}

impl Harness {
    async fn new(tag: &str, operator_handle: &str) -> Self {
        let dir = scratch_dir(tag);
        let mut config = Config::development_defaults();
        config.storage.root = dir.clone();
        let tdb = TestDb::connect_with_dir(tag, &dir).await;
        let db = tdb.db().clone();

        let mut bootstrap = TestClient::new(server::build_router(AppState::new(
            config.clone(),
            db.clone(),
        )));
        let (status, body) = bootstrap
            .post(
                "/api/v1/auth/register",
                json!({
                    "email": format!("{operator_handle}@example.com"),
                    "password": TEST_PASSWORD,
                    "handle": operator_handle,
                    "display_name": operator_handle,
                    "age_band": "adult",
                }),
            )
            .await;
        assert!(
            status.is_success(),
            "bootstrap register for {operator_handle}: {status} {body}"
        );
        drop(bootstrap);

        config.administration.operator_account_id =
            Some(account_of_handle(&db, operator_handle).await);
        Self {
            tdb,
            config,
            db,
            handle: operator_handle.to_owned(),
            _dir: dir,
        }
    }

    fn client(&self) -> TestClient {
        TestClient::new(server::build_router(AppState::new(
            self.config.clone(),
            self.db.clone(),
        )))
    }

    /// Seed `works` shown to the operator, of which the first `finished` were finished.
    async fn seed_impressions(&self, shown: usize, finished: usize) {
        let (account, pseud) = account_and_pseud(&self.db, &self.handle).await;
        // Seeded **one minute ago**, not now: the window is half-open `[since, until)`, so an
        // impression served in the same second the request arrives is outside it. That is correct
        // for the metric (a slot cannot be a hit before it is served) and is exactly the kind of
        // boundary a fixture has to respect rather than paper over.
        let served_at = minutes_ago(1);
        for i in 0..shown {
            let work = uuid::Uuid::new_v4().to_string();
            insert(
                &self.db,
                "INSERT INTO works (id, owner_pseud_id, title, lifecycle, visibility, created_at, \
                 updated_at, generated_content_posture) \
                 VALUES (?1#u, ?2#u, ?3, 'published', 'public', '2026-01-01T00:00:00Z', \
                 '2026-01-01T00:00:00Z', 'forbid')",
                &[&work, &pseud, &format!("Shown {i}")],
            )
            .await;
            insert(
                &self.db,
                "INSERT INTO recommendation_slots (id, pseud_id, work_id, request_id, position, \
                 reasons, blend_score, created_at) \
                 VALUES (?1#u, ?2#u, ?3#u, ?4#u, ?5, '[]', 0, ?6)",
                &[
                    &uuid::Uuid::new_v4().to_string(),
                    &pseud,
                    &work,
                    &format!("req-{i}"),
                    &i.to_string(),
                    &served_at,
                ],
            )
            .await;
            if i < finished {
                insert(
                    &self.db,
                    "INSERT INTO reading_status (id, account_id, subject_type, subject_id, status, \
                     updated_at) VALUES (?1#u, ?2#u, 'work', ?3#u, 'finished', ?4)",
                    &[
                        &uuid::Uuid::new_v4().to_string(),
                        &account,
                        &work,
                        &now_rfc3339(),
                    ],
                )
                .await;
            }
        }
    }
}

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("format now")
}

fn minutes_ago(minutes: i64) -> String {
    (time::OffsetDateTime::now_utc() - time::Duration::minutes(minutes))
        .format(&time::format_description::well_known::Rfc3339)
        .expect("format minutes ago")
}

/// One statement, both dialects. `?N#u` names a uuid; SQLite drops the cast.
async fn insert(db: &darkhold_db::Database, sqlite: &str, args: &[&str]) {
    use darkhold_db::Backend;
    match db.backend() {
        Backend::Sqlite => {
            let sql = sqlite.replace("#u", "");
            let mut q = sqlx::query(&sql);
            for a in args {
                q = q.bind(*a);
            }
            q.execute(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("seed");
        }
        Backend::Postgres => {
            let sql = (1..=6).fold(sqlite.to_string(), |acc, n| {
                acc.replace(&format!("?{n}#u"), &format!("${n}::uuid"))
                    .replace(&format!("?{n}"), &format!("${n}"))
            });
            let mut q = sqlx::query(&sql);
            for a in args {
                match uuid::Uuid::parse_str(a) {
                    Ok(u) => q = q.bind(u),
                    Err(_) => match a.parse::<i64>() {
                        Ok(n) => q = q.bind(n),
                        Err(_) => q = q.bind(*a),
                    },
                };
            }
            q.execute(db.postgres_pool().expect("postgres"))
                .await
                .expect("seed");
        }
    }
}

async fn account_of_handle(
    db: &darkhold_db::Database,
    handle: &str,
) -> darkhold_domain::ids::AccountId {
    account_and_pseud(db, handle)
        .await
        .0
        .parse()
        .expect("account id")
}

/// (account id, first pseud id) for a handle. Both are fetched as TEXT and cast per dialect, because
/// `account_id` is `TEXT` on SQLite and `uuid` on PostgreSQL.
async fn account_and_pseud(db: &darkhold_db::Database, handle: &str) -> (String, String) {
    use darkhold_db::Backend;
    let (sql, pool_is_pg) = match db.backend() {
        Backend::Sqlite => (
            "SELECT account_id, id FROM pseuds WHERE lower(handle) = lower(?) ORDER BY created_at",
            false,
        ),
        Backend::Postgres => (
            "SELECT account_id::text, id::text FROM pseuds WHERE lower(handle) = lower($1) \
             ORDER BY created_at",
            true,
        ),
    };
    if pool_is_pg {
        let row: (String, String) = sqlx::query_as(sql)
            .bind(handle)
            .fetch_one(db.postgres_pool().expect("postgres"))
            .await
            .expect("account and pseud");
        row
    } else {
        let row: (String, String) = sqlx::query_as(sql)
            .bind(handle)
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("account and pseud");
        row
    }
}

/// The operator reads their own number, and the window it was read over comes back with it.
#[tokio::test]
async fn the_operator_reads_their_own_hit_rate() {
    let h = Harness::new("hr_owner", "hitrate_op").await;
    h.seed_impressions(3, 2).await;

    let mut operator = h.client();
    sign_in_as(
        &mut operator,
        &h.tdb,
        "hitrate_op@example.com",
        "hitrate_op",
    )
    .await;
    let (status, body) = operator.get(&format!("{DOOR}?window_days=7")).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    assert_eq!(body["shown"], json!(3), "three works were served: {body}");
    assert_eq!(body["hits"], json!(2), "two were finished: {body}");
    assert_eq!(body["defined"], json!(true), "{body}");
    let rate = body["rate"].as_f64().expect("rate");
    assert!(
        (rate - 2.0 / 3.0).abs() < 1e-9,
        "hits/shown, got {rate} in {body}"
    );
    assert_eq!(
        body["window_days"],
        json!(7),
        "the window is echoed: {body}"
    );
    assert!(
        body["since"].as_i64().unwrap() < body["until"].as_i64().unwrap(),
        "the window has an order: {body}"
    );
}

/// No impressions is **undefined**, not zero, and the response says which.
#[tokio::test]
async fn an_empty_window_is_undefined_rather_than_zero() {
    let h = Harness::new("hr_empty", "hitrate_empty").await;

    let mut operator = h.client();
    sign_in_as(
        &mut operator,
        &h.tdb,
        "hitrate_empty@example.com",
        "hitrate_empty",
    )
    .await;
    let (status, body) = operator.get(DOOR).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["shown"], json!(0), "{body}");
    assert_eq!(body["hits"], json!(0), "{body}");
    assert_eq!(
        body["rate"],
        json!(null),
        "§53.5: an untested recipe has no rate, and 0.0 reads as total failure: {body}"
    );
    assert_eq!(body["defined"], json!(false), "{body}");
    assert_eq!(body["window_days"], json!(30), "the default window: {body}");
}

/// A non-operator gets `404` — the door must not confirm that it exists.
#[tokio::test]
async fn a_non_operator_gets_404() {
    let h = Harness::new("hr_404", "hitrate_boss").await;
    let mut intruder = h.client();
    sign_in_as(
        &mut intruder,
        &h.tdb,
        "not-the-operator@example.com",
        "hitrate_nobody",
    )
    .await;

    let (status, body) = intruder.get(DOOR).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "an operator surface answers 404 to anyone else: {body}"
    );
}

/// The surface is an **extension** (§59.3): an instance that does not run it answers exactly like one
/// where the caller is not the operator, so a probe cannot map the operator's configuration from
/// outside. Disabling goes through `[modules] disabled`, which is the mechanism every optional surface
/// uses — a second, feature-local switch would be a second thing to keep true.
#[tokio::test]
async fn a_disabled_extension_is_indistinguishable_from_a_refusal() {
    let mut h = Harness::new("hr_off", "hitrate_off").await;
    h.config.modules.disabled = vec!["operator_hit_rate".to_owned()];

    let mut operator = h.client();
    sign_in_as(
        &mut operator,
        &h.tdb,
        "hitrate_off@example.com",
        "hitrate_off",
    )
    .await;
    let (status, body) = operator.get(DOOR).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "the extension is off, and says nothing about why: {body}"
    );
    assert_eq!(
        body["error"]["code"], "MODULE_UNAVAILABLE",
        "the gate names the module without naming the reason: {body}"
    );
}

/// The window is clamped, not taken from the caller: an unbounded window turns a rate into a lifetime
/// average that no recipe change can move.
#[tokio::test]
async fn the_window_is_clamped_to_a_year() {
    let h = Harness::new("hr_clamp", "hitrate_clamp").await;
    h.seed_impressions(1, 0).await;

    let mut operator = h.client();
    sign_in_as(
        &mut operator,
        &h.tdb,
        "hitrate_clamp@example.com",
        "hitrate_clamp",
    )
    .await;
    let (status, body) = operator.get(&format!("{DOOR}?window_days=100000")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["window_days"], json!(365), "{body}");

    let (status, body) = operator.get(&format!("{DOOR}?window_days=0")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["window_days"], json!(1), "{body}");
}
