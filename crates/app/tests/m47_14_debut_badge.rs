//! M47-14 — §9.7.6's debut badge (audit item 75) over HTTP.
//!
//! What only the route and the publish path can get wrong:
//!
//! 1. **A first publication awards First Words, with its 20 credits**, on the author's *account*
//!    rather than their pseud: a reader who adds a second identity does not get a second badge.
//! 2. **A second publication awards nothing** — the badge is one-time, the ledger is not paid twice,
//!    and the profile stops calling the author new.
//! 3. **"Debut" is derived**, so it stops being true when the second work lands rather than when
//!    somebody remembers to clear a flag.
//!
//! The badge is written AFTER the publication commits, so a badge or ledger failure cannot roll back
//! a publication the author was already told succeeded.
//!
//! Both engines: `TestDb` is SQLite by default and PostgreSQL under `DARKHOLD_TEST_PG_URL`.

use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::DatabaseConfig;
use serde_json::{json, Value};
use test_support::TestDb;
use tower::ServiceExt;

const PASSWORD: &str = "test-pass-1234";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-m47-14-{tag}-{}-{:?}",
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

    fn cookie_header(&self) -> Option<String> {
        if self.cookies.is_empty() {
            return None;
        }
        Some(
            self.cookies
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect::<Vec<String>>()
                .join("; "),
        )
    }

    fn capture(&mut self, response: &axum::response::Response) {
        for value in response.headers().get_all(header::SET_COOKIE) {
            let Ok(text) = value.to_str() else { continue };
            let Some((pair, _)) = text.split_once(';') else {
                continue;
            };
            let Some((name, value)) = pair.split_once('=') else {
                continue;
            };
            let name = name.trim().to_owned();
            let value = value.trim().to_owned();
            self.cookies.retain(|(k, _)| k != &name);
            if !value.is_empty() {
                self.cookies.push((name, value));
            }
        }
    }

    async fn request(
        &mut self,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(cookie) = self.cookie_header() {
            builder = builder.header(header::COOKIE, cookie);
        }
        if let Some(token) = self
            .cookies
            .iter()
            .find(|(k, _)| k == "darkhold_csrf")
            .map(|(_, v)| v.to_owned())
        {
            builder = builder.header("x-csrf-token", token);
        }
        let body = match body {
            Some(v) => Body::from(v.to_string()),
            None => Body::empty(),
        };
        let response = self
            .app
            .clone()
            .oneshot(builder.body(body).unwrap())
            .await
            .unwrap();
        let status = response.status();
        self.capture(&response);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, json)
    }

    async fn get(&mut self, uri: &str) -> (StatusCode, Value) {
        self.request("GET", uri, None).await
    }

    async fn post(&mut self, uri: &str, body: Value) -> (StatusCode, Value) {
        self.request("POST", uri, Some(body)).await
    }

    async fn patch(&mut self, uri: &str, body: Value) -> (StatusCode, Value) {
        self.request("PATCH", uri, Some(body)).await
    }

    async fn register(&mut self, handle: &str) -> String {
        let (status, body) = self
            .post(
                "/api/v1/auth/register",
                json!({
                    "email": format!("m47-14-{handle}@example.test"),
                    "password": PASSWORD,
                    "handle": handle,
                    "display_name": handle,
                    "age_band": "adult",
                }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "register {handle}: {body}");
        body["pseuds"][0]["handle"]
            .as_str()
            .unwrap_or(handle)
            .to_owned()
    }

    /// Create a work with one written chapter and publish it. Returns the work id.
    async fn publish_a_work(&mut self, title: &str) -> String {
        let (status, body) = self.post("/api/v1/works", json!({ "title": title })).await;
        assert_eq!(status, StatusCode::CREATED, "create {title}: {body}");
        let work_id = body["id"].as_str().expect("work id").to_owned();
        let work_version = body["version"].as_i64().expect("work version");

        let (status, body) = self
            .post(
                &format!("/api/v1/works/{work_id}/chapters"),
                json!({ "title": "Chapter One" }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "create the chapter: {body}");
        let unit_id = body["id"].as_str().expect("unit id").to_owned();
        let unit_version = body["version"].as_i64().expect("unit version");

        let document = json!({ "type": "doc", "content": [
            { "type": "paragraph", "content": [{ "type": "text", "text": "Something worth reading." }] },
        ] });
        let (status, body) = self
            .patch(
                &format!("/api/v1/chapters/{unit_id}"),
                json!({ "expected_version": unit_version, "document": document }),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "write the chapter: {body}");

        let (status, body) = self
            .post(
                &format!("/api/v1/works/{work_id}/publish"),
                json!({ "expected_version": work_version, "idempotency_key": format!("m47-14-{work_id}") }),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "publish {title}: {body}");
        work_id
    }
}

struct Harness {
    _dir: PathBuf,
    tdb: TestDb,
    config: Config,
}

impl Harness {
    async fn new(tag: &str) -> Self {
        server::set_trust_proxy(false);
        let dir = scratch_dir(tag);
        let tdb = TestDb::connect_with_dir(tag, &dir).await;
        let config = config_for(&dir);
        Self {
            _dir: dir,
            tdb,
            config,
        }
    }

    fn client(&self) -> Client {
        Client::new(server::build_router(AppState::new(
            self.config.clone(),
            self.tdb.db().clone(),
        )))
    }

    /// The account id behind a pseud handle, straight from the database.
    async fn account_for(&self, handle: &str) -> String {
        // `account_id::text` on PostgreSQL: a UUID column will not decode into a `String`, and the
        // helper compares it against the TEXT buckets the ledger uses.
        let sql = self.tdb.sql(&format!(
            "SELECT account_id FROM pseuds WHERE handle = '{handle}'"
        ));
        let sql = if matches!(self.tdb.db().backend(), darkhold_db::Backend::Postgres) {
            sql.replace("SELECT account_id FROM", "SELECT account_id::text FROM")
        } else {
            sql
        };
        match self.tdb.db().backend() {
            darkhold_db::Backend::Sqlite => sqlx::query_scalar(&sql)
                .fetch_one(self.tdb.db().sqlite_pool().expect("sqlite"))
                .await
                .expect("the account"),
            darkhold_db::Backend::Postgres => sqlx::query_scalar(&sql)
                .fetch_one(self.tdb.db().postgres_pool().expect("pg"))
                .await
                .expect("the account"),
        }
    }
}

/// The granted credit balance for an account.
async fn granted(db: &darkhold_db::Database, account: &str) -> i64 {
    let balances = darkhold_db::economy::balances(db, account)
        .await
        .expect("balances");
    balances
        .into_iter()
        .find(|(bucket, _)| bucket == "granted")
        .map_or(0, |(_, amount)| amount)
}

#[tokio::test]
async fn a_first_publication_awards_first_words_with_its_bonus() {
    let h = Harness::new("debut").await;
    let mut author = h.client();
    let handle = author.register("author").await;
    author.publish_a_work("First Work").await;

    let (status, body) = author
        .get(&format!("/api/v1/pseuds/{handle}/profile"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["debut"],
        json!(true),
        "one published work is a debut: {body}"
    );
    let badges = body["badges"].as_array().expect("badges");
    assert_eq!(badges.len(), 1, "exactly one badge: {body}");
    assert_eq!(badges[0]["badge"], json!("first_words"));
    assert_eq!(badges[0]["name"], json!("First Words"));
    // §9.7.6's table.
    assert_eq!(badges[0]["credits"], json!(20));

    let account = h.account_for(&handle).await;
    assert_eq!(
        granted(h.tdb.db(), &account).await,
        20,
        "the bonus is paid once, to the account"
    );
}

#[tokio::test]
async fn a_second_publication_awards_nothing_and_ends_the_debut() {
    let h = Harness::new("second").await;
    let mut author = h.client();
    let handle = author.register("author").await;

    author.publish_a_work("First Work").await;
    author.publish_a_work("Second Work").await;

    let (status, body) = author
        .get(&format!("/api/v1/pseuds/{handle}/profile"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["debut"],
        json!(false),
        "derived, so a second work ends it without a flag to clear: {body}"
    );
    assert_eq!(
        body["badges"].as_array().expect("badges").len(),
        1,
        "one-time means one row: {body}"
    );

    let account = h.account_for(&handle).await;
    assert_eq!(
        granted(h.tdb.db(), &account).await,
        20,
        "a second publication pays no second bonus"
    );
}

#[tokio::test]
async fn an_author_who_has_not_published_has_no_badge_and_is_not_a_debut() {
    let h = Harness::new("none").await;
    let mut reader = h.client();
    let handle = reader.register("reader").await;

    let (status, body) = reader
        .get(&format!("/api/v1/pseuds/{handle}/profile"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["debut"], json!(false));
    assert!(
        body["badges"].as_array().expect("badges").is_empty(),
        "no badge before the condition is met: {body}"
    );
}
