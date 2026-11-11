//! Acceptance: the trust trail (M70-13).
//!
//! `trust_events::record` is the one writer and the table has existed since the ladder did; `list_all` was reached by nobody, so the
//! operator surface could **set** a level through `POST /admin/accounts/{id}/trust` without ever being able to say what had happened to one.
//! "Why is this account at level 4?" is the first question a dispute asks, and it had no answer that did not involve psql.

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
            if let Some((_, token)) = self.cookies.iter().find(|(name, _)| name == "darkhold_csrf") {
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

    /// Sign the operator in on a fresh client, so the session carries the operator's account.
    async fn signed_in(&self) -> Client {
        let mut client = self.operator();
        let (status, body) = client
            .send(
                "POST",
                "/api/v1/auth/login",
                Some(json!({ "email": "operator@example.com", "password": GOOD_PASSWORD })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "login: {body}");
        client
    }
}

/// The operator's account id.
///
/// Looked up by handle and not `LIMIT 1`: on PostgreSQL the scratch database is shared across the suite, so the first account
/// in the table belongs to whichever test ran first — which is exactly the kind of "works on SQLite" bug a two-engine run
/// exists to catch, and it caught this one on the first run.
async fn sqlx_enough(db: &Database) -> (String,) {
    match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_as(
            "SELECT account_id FROM pseuds WHERE handle = 'operator' LIMIT 1",
        )
            .fetch_one(db.sqlite_pool().expect("sqlite handle"))
            .await
            .expect("one account"),
        darkhold_db::Backend::Postgres => sqlx::query_as(
            "SELECT account_id::text FROM pseuds WHERE handle = 'operator' LIMIT 1",
        )
            .fetch_one(db.postgres_pool().expect("postgres handle"))
            .await
            .expect("one account"),
    }
}


#[tokio::test]
async fn an_operator_reads_every_trust_change_with_its_reason() {
    let harness = Harness::with_operator("trail").await;
    let mut client = harness.signed_in().await;

    let (account,): (String,) = {
        use darkhold_db::Backend;
        match harness.db().backend() {
            Backend::Sqlite => {
                sqlx::query_as("SELECT account_id FROM pseuds WHERE handle = 'operator' LIMIT 1")
                    .fetch_one(harness.db().sqlite_pool().expect("sqlite"))
                    .await
                    .expect("operator account")
            }
            Backend::Postgres => {
                sqlx::query_as("SELECT account_id::text FROM pseuds WHERE handle = 'operator' LIMIT 1")
                    .fetch_one(harness.db().postgres_pool().expect("postgres"))
                    .await
                    .expect("operator account")
            }
        }
    };
    darkhold_db::trust_events::record(
        harness.db(),
        &account,
        1,
        5,
        darkhold_db::trust_events::TrustCause::Operator,
        Some("ticket-42"),
        None,
    )
    .await
    .expect("record a trust change");

    let (status, body) = client.get("/api/v1/admin/trust-events").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let events = body["events"].as_array().expect("events");
    assert_eq!(events.len(), 1, "{body}");
    assert_eq!(events[0]["from_level"], 1);
    assert_eq!(events[0]["to_level"], 5);
    assert_eq!(
        events[0]["evidence_ref"], "ticket-42",
        "the row says what it was about, not just that something moved: {body}"
    );
    assert!(
        !events[0]["cause"].as_str().unwrap_or_default().is_empty(),
        "and why: {body}"
    );
    assert!(
        body["next"]["after"].is_string(),
        "the page carries the cursor for the next one: {body}"
    );

    // A stranger is told the door does not exist, because an operator trail that admits to existing is a map.
    let mut stranger = harness.operator();
    stranger
        .send(
            "POST",
            "/api/v1/auth/register",
            Some(json!({
                "email": "stranger@example.test",
                "password": GOOD_PASSWORD,
                "handle": "Stranger",
                "display_name": "Stranger",
                "age_band": "adult",
            })),
        )
        .await;
    let (status, refused) = stranger.get("/api/v1/admin/trust-events").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{refused}");
}

#[tokio::test]
async fn half_a_cursor_is_refused_rather_than_silently_ignored() {
    let harness = Harness::with_operator("cursor").await;
    let mut client = harness.signed_in().await;
    let (status, body) = client.get("/api/v1/admin/trust-events?after=2026-01-01T00:00:00Z").await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a cursor is two halves; taking one would page from the wrong place: {body}"
    );
}
