//! Acceptance: a submitter can see what happened to their adapter (M70-13, wiring `source_adapters::submissions_by`).
//!
//! `POST /extensions/source-adapters` has existed since the registry did, and `GET` on the same path lists what is **pending** for
//! reviewers — so a curator could file an adapter and then never learn the outcome, including the `reason` a rejection recorded. The store
//! function that answers it existed the whole time and only tests reached it.

use std::path::Path;

use axum::http::{header, Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::Database;
use serde_json::{json, Value};
use test_support::TestDb;
use tower::ServiceExt;

const GOOD_PASSWORD: &str = "a-long-enough-passphrase";

fn scratch_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-m69-23-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn config_for(dir: &Path) -> Config {
    let mut config = Config::development_defaults();
    config.storage.root = dir.to_path_buf();
    config.database = match std::env::var("DARKHOLD_TEST_PG_URL") {
        Ok(url) => darkhold_db::DatabaseConfig::new(url),
        Err(_) => darkhold_db::DatabaseConfig::new(format!(
            "sqlite://{}/darkhold.sqlite?mode=rwc",
            dir.display()
        )),
    };
    config.rate_limits.auth = darkhold_app::limiter::Quota {
        burst: 1000,
        per_minute: 6000,
    };
    config.rate_limits.write = darkhold_app::limiter::Quota {
        burst: 1000,
        per_minute: 6000,
    };
    config.rate_limits.default = darkhold_app::limiter::Quota {
        burst: 1000,
        per_minute: 6000,
    };
    config
}

struct Client {
    app: axum::Router,
    cookies: Vec<(String, String)>,
}

impl Client {
    fn cookie_header(&self) -> String {
        self.cookies
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; ")
    }

    fn capture(&mut self, response: &axum::response::Response) {
        for value in response.headers().get_all(header::SET_COOKIE) {
            let Ok(raw) = value.to_str() else { continue };
            let pair = raw.split(';').next().unwrap_or(raw);
            let Some((name, val)) = pair.split_once('=') else {
                continue;
            };
            let (name, val) = (name.to_owned(), val.to_owned());
            self.cookies.retain(|(key, _)| key != &name);
            if !val.is_empty() {
                self.cookies.push((name, val));
            }
        }
    }

    async fn send(&mut self, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut builder = Request::builder().method(method).uri(path);
        if !self.cookies.is_empty() {
            builder = builder.header(header::COOKIE, self.cookie_header());
        }
        if !matches!(method, "GET" | "HEAD" | "OPTIONS") {
            if let Some((_, token)) = self
                .cookies
                .iter()
                .find(|(name, _)| name == "darkhold_csrf")
            {
                let token = token.clone();
                builder = builder.header("x-csrf-token", token);
            }
        }
        if body.is_some() {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
        }
        let request = match body {
            Some(body) => builder.body(axum::body::Body::from(body.to_string())),
            None => builder.body(axum::body::Body::empty()),
        }
        .expect("request");
        let response = self
            .app
            .clone()
            .oneshot(request)
            .await
            .expect("route response");
        let status = response.status();
        self.capture(&response);
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("response body");
        let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, value)
    }

    async fn get(&mut self, path: &str) -> (StatusCode, Value) {
        self.send("GET", path, None).await
    }
}

struct Harness {
    _tdb: TestDb,
    _dir: std::path::PathBuf,
    config: Config,
    db: Database,
}

impl Harness {
    /// A harness whose `operator` account is the configured operator, with a source row to carry a policy.
    async fn with_operator(tag: &str) -> Self {
        let dir = scratch_dir(tag);
        let mut config = config_for(&dir);
        let tdb = TestDb::connect_with_dir(tag, &dir).await;
        let db = tdb.db().clone();

        let mut bootstrap = Client {
            app: server::build_router(AppState::new(config.clone(), db.clone())),
            cookies: Vec::new(),
        };
        let (status, body) = bootstrap
            .send(
                "POST",
                "/api/v1/auth/register",
                Some(json!({
                    "email": "operator@example.com",
                    "password": GOOD_PASSWORD,
                    "handle": "operator",
                    "display_name": "operator",
                    "age_band": "adult",
                })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "bootstrap register: {body}");

        let (account,): (String,) = sqlx_enough(&db).await;
        config.administration.operator_account_id =
            Some(account.parse().expect("account id parses"));

        darkhold_db::imports::upsert_source(&db, "ao3", "Archive of Our Own", "1.0.0", "{}")
            .await
            .expect("seed a source");

        Self {
            _tdb: tdb,
            _dir: dir,
            config,
            db,
        }
    }

    fn operator(&self) -> Client {
        Client {
            app: server::build_router(AppState::new(self.config.clone(), self.db.clone())),
            cookies: Vec::new(),
        }
    }

    fn db(&self) -> &Database {
        &self.db
    }
}

/// The operator's account id.
///
/// Looked up by handle and not `LIMIT 1`: on PostgreSQL the scratch database is shared across the suite, so the first account
/// in the table belongs to whichever test ran first — which is exactly the kind of "works on SQLite" bug a two-engine run
/// exists to catch, and it caught this one on the first run.
async fn sqlx_enough(db: &Database) -> (String,) {
    match db.backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query_as("SELECT account_id FROM pseuds WHERE handle = 'operator' LIMIT 1")
                .fetch_one(db.sqlite_pool().expect("sqlite handle"))
                .await
                .expect("one account")
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query_as("SELECT account_id::text FROM pseuds WHERE handle = 'operator' LIMIT 1")
                .fetch_one(db.postgres_pool().expect("postgres handle"))
                .await
                .expect("one account")
        }
    }
}

#[tokio::test]
async fn a_submitter_reads_their_own_submission_and_nobody_else_does() {
    let harness = Harness::with_operator("mine").await;
    let mut mine = harness.operator();
    let (status, me) = mine
        .send(
            "POST",
            "/api/v1/auth/register",
            Some(json!({
                "email": "submitter@example.test",
                "password": GOOD_PASSWORD,
                "handle": "Submitter",
                "display_name": "Submitter",
                "age_band": "adult",
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{me}");
    let account = me["account"]["id"].as_str().expect("account id").to_owned();
    darkhold_db::governance::set_trust(
        harness.db(),
        &account,
        darkhold_db::source_adapters::SUBMIT_TRUST_BAR,
        "test",
    )
    .await
    .expect("grant the submit bar");

    let (status, filed) = mine
        .send(
            "POST",
            "/api/v1/extensions/source-adapters",
            Some(json!({ "manifest": "id: mine-test" })),
        )
        .await;
    assert!(
        status == StatusCode::OK || status == StatusCode::CREATED,
        "submit: {status} {filed}"
    );

    let (status, list) = mine.get("/api/v1/extensions/source-adapters/mine").await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let rows = list["mine"].as_array().expect("mine");
    assert_eq!(
        rows.len(),
        1,
        "the submitter's one submission, and only theirs: {list}"
    );
    assert_eq!(rows[0]["state"], "pending", "{list}");
    assert_eq!(rows[0]["kind"], "extension", "{list}");

    // Somebody else's list is empty, not everybody's: the door is scoped by account, not filtered afterwards.
    let mut other = harness.operator();
    other
        .send(
            "POST",
            "/api/v1/auth/register",
            Some(json!({
                "email": "other@example.test",
                "password": GOOD_PASSWORD,
                "handle": "Other",
                "display_name": "Other",
                "age_band": "adult",
            })),
        )
        .await;
    let (status, theirs) = other.get("/api/v1/extensions/source-adapters/mine").await;
    assert_eq!(status, StatusCode::OK, "{theirs}");
    assert_eq!(
        theirs["mine"].as_array().expect("mine").len(),
        0,
        "another account's submission must not appear in this list: {theirs}"
    );
}
