//! M45-45 — a lens change through the door: open, vote, and the read that applies it.
//!
//! The store suite proves the counting. What only the routes can get wrong:
//!
//! 1. **An anonymous vote is refused** — a vote is one per account, so an account is what a session is.
//! 2. **A reader cannot open a proposal** — opening decides what the instance will be asked; that is the operator's.
//! 3. **A second vote replaces the first through the door**, not only in the store.
//! 4. **A due proposal is applied by the read**, and the version records `cause: proposal:<id>`.

use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use darkhold_app::config::{Config, LensOwner};
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::{Database, DatabaseConfig};
use serde_json::{json, Value};
use test_support::TestDb;
use tower::ServiceExt;

const PASSWORD: &str = "test-pass-1234";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-m45-45-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn config_for(dir: &Path) -> Config {
    let mut config = Config::development_defaults();
    config.storage.root = dir.to_path_buf();
    config.database = match std::env::var("DARKHOLD_TEST_PG_URL") {
        Ok(url) if !url.is_empty() => DatabaseConfig::new(url),
        _ => DatabaseConfig::new(format!(
            "sqlite://{}/darkhold.sqlite?mode=rwc",
            dir.display()
        )),
    };
    // The mode this suite is about: a direct write is refused under it, so this door is the only way in.
    config.taste.lens_owner = LensOwner::Vote;
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
        if !self.cookies.is_empty() {
            let cookie = self
                .cookies
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect::<Vec<String>>()
                .join("; ");
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
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn get(&mut self, uri: &str) -> (StatusCode, Value) {
        self.request("GET", uri, None).await
    }

    async fn send(&mut self, method: &str, uri: &str, body: Value) -> (StatusCode, Value) {
        self.request(method, uri, Some(body)).await
    }

    async fn register(&mut self, handle: &str) {
        let (status, body) = self
            .send(
                "POST",
                "/api/v1/auth/register",
                json!({
                    "email": format!("m45-45-{handle}@example.test"),
                    "password": PASSWORD,
                    "handle": handle,
                    "display_name": handle,
                    "age_band": "adult",
                }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "register {handle}: {body}");
    }
}

struct Harness {
    _dir: PathBuf,
    tdb: TestDb,
    config: Config,
}

impl Harness {
    /// A harness whose first account is the operator.
    async fn new(tag: &str) -> Self {
        server::set_trust_proxy(false);
        let dir = scratch_dir(tag);
        let mut config = config_for(&dir);
        let tdb = TestDb::connect_with_dir(tag, &dir).await;
        let db = tdb.db().clone();

        let mut bootstrap = Client::new(server::build_router(AppState::new(
            config.clone(),
            db.clone(),
        )));
        bootstrap.register("operator").await;
        let account = account_of(&db, "operator").await;
        config.administration.operator_account_id = Some(account.into());

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

    /// A signed-in client. `operator` is signed in as the operator account; anything else registers a fresh reader.
    async fn signed_in(&self, handle: &str) -> Client {
        if handle == "operator" {
            let mut client = self.client();
            let (status, body) = client
                .send(
                    "POST",
                    "/api/v1/auth/login",
                    json!({ "email": "m45-45-operator@example.test", "password": PASSWORD }),
                )
                .await;
            assert_eq!(status, StatusCode::OK, "log the operator back in: {body}");
            return client;
        }
        let mut client = self.client();
        client.register(handle).await;
        client
    }

    fn db(&self) -> &Database {
        self.tdb.db()
    }
}

fn proposal_body() -> Value {
    json!({
        "gravity_strength": 3000,
        "signal_weight_mode": "balanced",
        "admin_weight": 5000,
        "diversity_injection_percent": 2500,
        "closes_in_days": 7,
        "threshold_percent": 60,
    })
}

async fn opened(operator: &mut Client) -> String {
    let (status, body) = operator
        .send("POST", "/api/v1/taste-proposals", proposal_body())
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the operator opens a proposal: {body}"
    );
    body["proposal"]["id"]
        .as_str()
        .expect("a proposal id")
        .to_owned()
}

#[tokio::test]
async fn a_reader_cannot_open_a_proposal() {
    let harness = Harness::new("no-open").await;
    let mut reader = harness.signed_in("reader").await;

    let (status, body) = reader
        .send("POST", "/api/v1/taste-proposals", proposal_body())
        .await;

    // `404`, not `403`: the operator surface is concealed (ADR 0003, `require_estate_hidden`), so a reader does not learn
    // that this door exists. The refusal is a feature, and asserting `403` here would demand that the instance disclose it.
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "opening decides what the instance is asked: {body}"
    );
}

#[tokio::test]
async fn an_anonymous_vote_is_refused() {
    let harness = Harness::new("anon").await;
    let mut operator = harness.signed_in("operator").await;
    let id = opened(&mut operator).await;

    let mut anonymous = harness.client();
    let (status, body) = anonymous
        .send(
            "POST",
            &format!("/api/v1/taste-proposals/{id}/vote"),
            json!({ "vote": "yes" }),
        )
        .await;

    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "one vote per account needs an account: {body}"
    );
}

#[tokio::test]
async fn a_second_vote_replaces_the_first_through_the_door() {
    let harness = Harness::new("replace").await;
    let mut operator = harness.signed_in("operator").await;
    let id = opened(&mut operator).await;
    let mut reader = harness.signed_in("reader2").await;

    let (status, body) = reader
        .send(
            "POST",
            &format!("/api/v1/taste-proposals/{id}/vote"),
            json!({ "vote": "yes" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "first vote: {body}");
    assert_eq!(body["tally"]["yes"], 1, "one yes: {body}");

    let (status, body) = reader
        .send(
            "POST",
            &format!("/api/v1/taste-proposals/{id}/vote"),
            json!({ "vote": "no" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "changed mind: {body}");
    assert_eq!(
        body["tally"]["yes"], 0,
        "the yes was replaced, not added: {body}"
    );
    assert_eq!(body["tally"]["no"], 1, "and the no counts once: {body}");
}

#[tokio::test]
async fn a_due_proposal_is_applied_by_the_read_and_says_the_proposal_did_it() {
    let harness = Harness::new("apply").await;
    let mut operator = harness.signed_in("operator").await;
    let id = opened(&mut operator).await;
    let mut reader = harness.signed_in("reader3").await;
    reader
        .send(
            "POST",
            &format!("/api/v1/taste-proposals/{id}/vote"),
            json!({ "vote": "yes" }),
        )
        .await;

    // Close it in the past: the window is policy, the decision is the clock's, and a test that waited seven days would not
    // be a test anybody runs.
    {
        let db = harness.db();
        let sql = db.sql(
            "UPDATE taste_lens_proposals SET closes_at = ? WHERE id = ?",
            "UPDATE taste_lens_proposals SET closes_at = $1 WHERE id = $2",
        );
        match db.backend() {
            darkhold_db::Backend::Sqlite => {
                sqlx::query(&sql)
                    .bind("2020-01-01T00:00:00Z")
                    .bind(&id)
                    .execute(db.sqlite_pool().expect("sqlite"))
                    .await
                    .expect("close");
            }
            darkhold_db::Backend::Postgres => {
                sqlx::query(&sql)
                    .bind("2020-01-01T00:00:00Z")
                    .bind(&id)
                    .execute(db.postgres_pool().expect("postgres"))
                    .await
                    .expect("close");
            }
        }
    }

    let (status, body) = reader.get(&format!("/api/v1/taste-proposals/{id}")).await;
    assert_eq!(status, StatusCode::OK, "read: {body}");
    assert_eq!(
        body["proposal"]["state"], "passed",
        "unanimous, past its close: {body}"
    );

    let history = darkhold_db::instance_taste_settings::history(harness.db(), 1)
        .await
        .expect("history");
    let latest = history.first().expect("a version was written");
    assert_eq!(
        latest.cause,
        format!("proposal:{id}"),
        "the history must say the proposal caused this change, not only who pressed the button"
    );
}

/// The account behind a handle: handles live on `pseuds`, not on `accounts`.
async fn account_of(db: &Database, handle: &str) -> uuid::Uuid {
    let sql = db.sql(
        "SELECT account_id FROM pseuds WHERE lower(handle) = lower(?)",
        "SELECT account_id::text AS account_id FROM pseuds WHERE lower(handle) = lower(?)",
    );
    let value: String = match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar(&sql)
            .bind(handle)
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("the account"),
        darkhold_db::Backend::Postgres => sqlx::query_scalar(&sql)
            .bind(handle)
            .fetch_one(db.postgres_pool().expect("pg"))
            .await
            .expect("the account"),
    };
    value.parse().expect("an account id is a uuid")
}
