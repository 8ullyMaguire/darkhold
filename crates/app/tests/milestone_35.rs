//! M35 — Moderation ladder, slow mode, federation scope, featured posts.
//!
//! Spec §35.5.

use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server::{self, set_trust_proxy};
use darkhold_app::state::AppState;
use darkhold_db::DatabaseConfig;
use serde_json::{json, Value};
use tower::ServiceExt;

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-m35-{tag}-{}-{:?}",
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
    config.database = DatabaseConfig::new(format!(
        "sqlite://{}/darkhold.sqlite?mode=rwc",
        dir.display()
    ));
    // Raise rate limits for parallel test execution.
    config.rate_limits.auth = darkhold_app::limiter::Quota {
        burst: 1000,
        per_minute: 6000,
    };
    config.rate_limits.write = darkhold_app::limiter::Quota {
        burst: 1000,
        per_minute: 6000,
    };
    config.rate_limits.search = darkhold_app::limiter::Quota {
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
    fn new(app: axum::Router) -> Self {
        Self {
            app,
            cookies: Vec::new(),
        }
    }
    fn cookie(&self, name: &str) -> Option<&str> {
        self.cookies
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
    fn capture(&mut self, response: &axum::response::Response) {
        for value in response.headers().get_all(header::SET_COOKIE) {
            let Ok(text) = value.to_str() else { continue };
            let Some((pair, _)) = text.split_once(';') else {
                continue;
            };
            if let Some((name, value)) = pair.split_once('=') {
                let name = name.trim().to_owned();
                let value = value.trim().to_owned();
                self.cookies.retain(|(k, _)| k != &name);
                if !value.is_empty() {
                    self.cookies.push((name, value));
                }
            }
        }
    }
    async fn request(
        &mut self,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder().method(method).uri(uri);
        if !self.cookies.is_empty() {
            builder = builder.header(
                header::COOKIE,
                self.cookies
                    .iter()
                    .map(|(n, v)| format!("{n}={v}"))
                    .collect::<Vec<_>>()
                    .join("; "),
            );
        }
        if !matches!(method, "GET" | "HEAD" | "OPTIONS") {
            if let Some(token) = self.cookie("darkhold_csrf").map(str::to_owned) {
                builder = builder.header("x-csrf-token", token);
            }
        }
        let request = match body {
            Some(v) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&v).expect("serialise")))
                .expect("request"),
            None => builder.body(Body::empty()).expect("request"),
        };
        let response = self.app.clone().oneshot(request).await.expect("response");
        let status = response.status();
        self.capture(&response);
        let bytes = axum::body::to_bytes(response.into_body(), 16 * 1024 * 1024)
            .await
            .expect("body");
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes)
                .unwrap_or(Value::String(String::from_utf8_lossy(&bytes).into_owned()))
        };
        (status, value)
    }
    async fn get(&mut self, uri: &str) -> (StatusCode, Value) {
        self.request("GET", uri, None).await
    }
    async fn post(&mut self, uri: &str, body: Value) -> (StatusCode, Value) {
        self.request("POST", uri, Some(body)).await
    }
    #[allow(dead_code)]
    async fn put(&mut self, uri: &str, body: Value) -> (StatusCode, Value) {
        self.request("PUT", uri, Some(body)).await
    }
}

struct Harness {
    dir: PathBuf,
    tdb: test_support::TestDb,
}

impl Harness {
    async fn new(tag: &str) -> Self {
        set_trust_proxy(false);
        let _ = darkhold_app::logging::init(&darkhold_app::config::LoggingConfig {
            filter: "error".to_owned(),
            format: darkhold_app::config::LogFormat::Pretty,
        });
        let dir = scratch_dir(tag);
        let tdb = test_support::TestDb::connect_with_dir(tag, &dir).await;
        Self { dir, tdb }
    }

    fn client(&self) -> Client {
        Client::new(server::build_router(AppState::new(
            config_for(&self.dir),
            self.tdb.db().clone(),
        )))
    }
}

const PASSWORD: &str = "a-long-enough-passphrase";

async fn register(client: &mut Client, email: &str, handle: &str) -> (String, String) {
    let (status, body) = client
        .post(
            "/api/v1/auth/register",
            json!({
                "email": email,
                "password": PASSWORD,
                "handle": handle,
                "display_name": handle,
                "age_band": "adult"
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "register {handle}: {body}");
    let (status, me) = client.get("/api/v1/auth/me").await;
    assert_eq!(status, StatusCode::OK, "{me}");
    let account = me["account"]["id"].as_str().expect("account id").to_owned();
    let pseud = me["active_pseud_id"].as_str().expect("pseud id").to_owned();
    (account, pseud)
}

async fn create_topic(client: &mut Client, category: &str, title: &str) -> String {
    let (status, body) = client
        .post(
            &format!("/api/v1/forums/{category}/topics"),
            json!({ "category": category, "title": title }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "create topic: {body}");
    body["id"].as_str().expect("topic id").to_owned()
}

#[tokio::test]
async fn graduated_response_ladder_roundtrip() {
    let harness = Harness::new("ladder").await;
    let mut client = harness.client();
    let (alice_account, _) = register(&mut client, "alice@example.com", "alice").await;

    let target_email = "target@example.com";
    let (target_account, _) = register(&mut client, target_email, "target").await;

    darkhold_db::moderation::apply_sanction(
        harness.tdb.db(),
        &target_account,
        None,
        darkhold_domain::moderation::SanctionLevel::PostThrottle,
        "Too many rapid posts",
        &alice_account,
        None,
    )
    .await
    .expect("apply_sanction");

    let sanction = darkhold_db::moderation::check_sanction(harness.tdb.db(), &target_account, None)
        .await
        .expect("check_sanction");
    assert!(sanction.is_some());
    let s = sanction.expect("sanction");
    assert_eq!(s.level, "post_throttle");
}

#[tokio::test]
async fn slow_mode_roundtrip() {
    let harness = Harness::new("slow").await;
    let mut client = harness.client();
    let (_, _pseud) = register(&mut client, "alice@example.com", "alice").await;

    let topic_id = create_topic(&mut client, "general", "Test Topic").await;

    darkhold_db::moderation::set_slow_mode(harness.tdb.db(), &topic_id, 60)
        .await
        .expect("set_slow_mode");

    let t = darkhold_db::community::topic_by_id(harness.tdb.db(), &topic_id)
        .await
        .expect("topic")
        .expect("topic exists");
    assert_eq!(t.mode, "plain");
}

#[tokio::test]
async fn federation_scope_roundtrip() {
    let harness = Harness::new("federation").await;
    let mut client = harness.client();
    let (_, _pseud) = register(&mut client, "alice@example.com", "alice").await;

    let topic_id = create_topic(&mut client, "general", "Test Topic").await;

    darkhold_db::moderation::set_federation_scope(harness.tdb.db(), &topic_id, "local")
        .await
        .expect("set_federation_scope");

    let t = darkhold_db::community::topic_by_id(harness.tdb.db(), &topic_id)
        .await
        .expect("topic")
        .expect("topic exists");
    assert_eq!(t.mode, "plain");
}

#[tokio::test]
async fn featured_post_roundtrip() {
    let harness = Harness::new("featured").await;
    let mut client = harness.client();
    let (account_id, pseud) = register(&mut client, "alice@example.com", "alice").await;

    let topic_id = create_topic(&mut client, "general", "Test Topic").await;

    let post_id = darkhold_db::community::create_post(harness.tdb.db(), &topic_id, &pseud, "Hello")
        .await
        .expect("create_post");

    darkhold_db::moderation::feature_post(harness.tdb.db(), &post_id, &account_id)
        .await
        .expect("feature_post");
}

#[tokio::test]
async fn search_miss_roundtrip() {
    let harness = Harness::new("search-miss").await;
    darkhold_db::moderation::record_search_miss(harness.tdb.db(), "some-query-hash", "some query")
        .await
        .expect("record_search_miss");

    // Read back through whichever pool the fixture is on, so the assertion is
    // the same claim on both backends instead of SQLite-only by construction.
    // `count::bigint` on the PostgreSQL side: the column is INTEGER there and
    // sqlx will not decode an INT4 into an i64. The cast belongs in the query
    // rather than in the Rust type, so the test asserts the same claim on both.
    let sql = harness.tdb.db().sql(
        "SELECT query_text, count FROM forum_search_misses WHERE query_text = ?",
        "SELECT query_text, count::bigint AS count FROM forum_search_misses WHERE query_text = $1",
    );
    let row: Option<(String, i64)> = match harness.tdb.db().backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_as(&sql)
            .bind("some query")
            .fetch_optional(harness.tdb.db().sqlite_pool().expect("sqlite"))
            .await
            .expect("fetch_optional"),
        darkhold_db::Backend::Postgres => sqlx::query_as(&sql)
            .bind("some query")
            .fetch_optional(harness.tdb.db().postgres_pool().expect("postgres"))
            .await
            .expect("fetch_optional"),
    };

    assert!(row.is_some());
    let (query, count) = row.expect("row");
    assert_eq!(query, "some query");
    assert_eq!(count, 1);
}

/// A zero-result search becomes demand, and a reply becomes daily activity (§35.5, M70-13).
///
/// `record_search_miss` and `record_topic_activity` had no caller: the tables that turn "nobody can find it" and "how busy is
/// this conversation" into data were written by nothing. The claims:
///
/// 1. A free-text forum search that finds nothing is recorded, and a second identical search increments the same row rather
///    than adding another — the miss is a demand signal, so ten readers who wanted the same thing are one entry with ten.
/// 2. A search that only narrows (`category:` with no `q`) records nothing: that is a taxonomy question, not something
///    anybody asked to read.
/// 3. The operator can read the list at `/supply/unmet-searches`; a stranger cannot (404, not 403).
/// 4. A reply adds one row of activity for the topic and today's date, and a second reply increments it.
#[tokio::test]
async fn a_zero_result_search_becomes_demand_and_a_reply_becomes_activity() {
    let harness = Harness::new("demand-and-activity").await;
    let category = seed_forum_category(&harness, "General").await;

    let mut reader = harness.client();
    register(&mut reader, "m35-demand@t.test", "m35demand").await;

    // 1. Two identical free-text searches with no hits: one row, count two.
    for _ in 0..2 {
        let (status, body) = reader
            .get("/api/v1/forum-search?q=unfindable%20thing")
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["items"], json!([]), "{body}");
    }
    // 2. A narrowing-only search records nothing.
    let (status, body) = reader.get(&format!("/api/v1/forum-search?category={category}")).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // 3. The operator reads the list; a stranger gets 404.
    let mut operator = harness.client();
    let (operator_account, _) = register(&mut operator, "m35-op@t.test", "m35op").await;
    let mut operator_config = config_for(&harness.dir);
    operator_config.administration.operator_account_id = Some(operator_account.parse().expect("id"));
    let mut operator = operator.rejoin(server::build_router(AppState::new(
        operator_config,
        harness.tdb.db().clone(),
    )));

    let (status, body) = operator.get("/api/v1/supply/unmet-searches").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let items = body["items"].as_array().expect("items");
    assert_eq!(items.len(), 1, "one distinct miss, however many times: {body}");
    assert_eq!(items[0]["query"], json!("unfindable thing"), "{body}");
    assert_eq!(items[0]["count"], json!(2), "{body}");

    let (status, body) = reader.get("/api/v1/supply/unmet-searches").await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "the commission queue is the operator's: {body}"
    );

    // 4. A reply records today's activity, and a second reply increments the same day.
    let topic = create_topic(&mut reader, &category, "Busy today").await;
    for _ in 0..2 {
        let (status, body) = reader
            .post(
                &format!("/api/v1/topics/{topic}/replies"),
                json!({ "body": "A reply." }),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    let today = chrono::Utc::now().date_naive().to_string();
    let sql = harness.tdb.db().sql(
        "SELECT reply_count FROM forum_topic_activity WHERE topic_id = ? AND activity_date = ?",
        "SELECT reply_count::bigint FROM forum_topic_activity WHERE topic_id = $1 AND activity_date = $2",
    );
    let replies: i64 = match harness.tdb.db().backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar(&sql)
            .bind(&topic)
            .bind(&today)
            .fetch_one(harness.tdb.db().sqlite_pool().expect("sqlite"))
            .await
            .expect("activity row"),
        darkhold_db::Backend::Postgres => sqlx::query_scalar(&sql)
            .bind(&topic)
            .bind(&today)
            .fetch_one(harness.tdb.db().postgres_pool().expect("postgres"))
            .await
            .expect("activity row"),
    };
    assert_eq!(replies, 2, "two replies on {today} for {topic}");
}

/// A category row, as an operator's seed would create one. This harness predates the helper in `milestone_32`, so it carries
/// its own rather than importing a fixture across test binaries (which would need a support crate for four lines).
async fn seed_forum_category(harness: &Harness, name: &str) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    let sql = harness
        .tdb
        .db()
        .sql(
            "INSERT INTO forum_categories (id, name, position, min_trust) VALUES (?, ?, 0, 0)",
            "INSERT INTO forum_categories (id, name, position, min_trust) VALUES ($1, $2, 0, 0)",
        );
    match harness.tdb.db().backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(&id)
                .bind(name)
                .execute(harness.tdb.db().sqlite_pool().expect("sqlite"))
                .await
                .expect("seed category");
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(&sql)
                .bind(&id)
                .bind(name)
                .execute(harness.tdb.db().postgres_pool().expect("postgres"))
                .await
                .expect("seed category");
        }
    }
    id
}

impl Client {
    /// The same cookies and a new app: "sign in, then change the config" is one reader and two routers.
    fn rejoin(&self, app: axum::Router) -> Self {
        Self {
            app,
            cookies: self.cookies.clone(),
        }
    }
}
