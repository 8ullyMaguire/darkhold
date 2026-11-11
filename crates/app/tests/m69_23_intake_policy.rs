//! Acceptance: an import source carries its intake policy (M69-23, Suki §8.4/§11.4).
//!
//! `crates/domain/src/intake.rs` proves the rule. This file proves the **row**: that a source can carry a policy, that the
//! policy survives a round trip through the store on both engines, and — the part the row exists for — that an operator can
//! ask what the policy would do to an arrival and get a *sentence* back rather than a boolean.
//!
//! Three things are asserted that a weaker test would miss:
//!
//! * **A policy that does not parse is refused before it is stored.** The stored form is what the importer runs, so
//!   `"then": "Bring"` must be a 422 with a field error and not a stored rule that silently matches nothing.
//! * **The dry run writes nothing.** It is run against a source whose policy admits, and afterwards the source's own row is
//!   unchanged — a "test" door that mutated state would be a second way to configure an importer.
//! * **An unknown source is a 404**, not a policy for a source nobody will ever read.

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

    async fn put(&mut self, path: &str, body: Value) -> (StatusCode, Value) {
        self.send("PUT", path, Some(body)).await
    }

    async fn post(&mut self, path: &str, body: Value) -> (StatusCode, Value) {
        self.send("POST", path, Some(body)).await
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

/// A policy that admits a long English AO3 work and queues anything tagged `dead dove`.
fn policy() -> Value {
    json!({
        "rules": [
            {
                "when": [
                    { "kind": "host_in", "value": ["archiveofourown.org"] },
                    { "kind": "language_in", "value": ["en"] },
                    { "kind": "words_at_least", "value": 1000 }
                ],
                "then": "bring_in"
            },
            {
                "when": [{ "kind": "tag_all", "value": ["dead dove"] }],
                "then": "queue"
            }
        ]
    })
}

#[tokio::test]
async fn a_policy_is_stored_and_read_back_with_its_rule_count() {
    let harness = Harness::with_operator("store").await;
    let mut client = harness.signed_in().await;

    let (status, saved) = client
        .put("/api/v1/admin/sources/ao3/intake-policy", policy())
        .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["rules"], 2);

    let (status, list) = client.get("/api/v1/admin/sources").await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let source = list["sources"]
        .as_array()
        .expect("sources")
        .iter()
        .find(|row| row["key"] == "ao3")
        .expect("ao3 in the list");
    assert_eq!(source["rules"], 2, "{source}");
    assert_eq!(
        source["policy"]["rules"][0]["then"], "bring_in",
        "the stored policy comes back parsed, not as a string an operator has to decode: {source}"
    );

    // And the store holds what the route said it did — the round trip is the claim, not the response body.
    let stored = darkhold_db::imports::intake_policy(harness.db(), "ao3")
        .await
        .expect("read policy")
        .expect("ao3 exists");
    assert!(stored.contains("bring_in"), "{stored}");
}

#[tokio::test]
async fn the_dry_run_answers_with_a_sentence_not_a_boolean() {
    let harness = Harness::with_operator("dryrun").await;
    let mut client = harness.signed_in().await;
    client
        .put("/api/v1/admin/sources/ao3/intake-policy", policy())
        .await;

    let (status, admitted) = client
        .post(
            "/api/v1/admin/sources/ao3/intake-policy/test",
            json!({ "candidate": {
                "host": "archiveofourown.org",
                "language": "en",
                "words": 4000,
                "title": "A Long Story"
            }}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{admitted}");
    assert_eq!(admitted["bring_in"], true, "{admitted}");
    assert!(
        admitted["decision"]
            .as_str()
            .unwrap_or_default()
            .starts_with("bring in: rule 0 matched"),
        "the operator reads why, not just yes: {admitted}"
    );

    let (status, unmatched) = client
        .post(
            "/api/v1/admin/sources/ao3/intake-policy/test",
            json!({ "candidate": { "host": "example.invalid", "words": 10 } }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{unmatched}");
    assert_eq!(unmatched["bring_in"], false, "{unmatched}");
    assert!(
        unmatched["decision"]
            .as_str()
            .unwrap_or_default()
            .contains("needs a person"),
        "an arrival no rule settles is queued for a person, and the sentence says so: {unmatched}"
    );

    // The dry run writes nothing: the policy is still the two rules it was.
    let (_, list) = client.get("/api/v1/admin/sources").await;
    let rules = list["sources"]
        .as_array()
        .expect("sources")
        .iter()
        .find(|row| row["key"] == "ao3")
        .expect("ao3")["rules"]
        .clone();
    assert_eq!(rules, 2, "a dry run is not a way to configure an importer");
}

#[tokio::test]
async fn a_policy_that_does_not_parse_is_refused_before_it_is_stored() {
    let harness = Harness::with_operator("parse").await;
    let mut client = harness.signed_in().await;

    let (status, refused) = client
        .put(
            "/api/v1/admin/sources/ao3/intake-policy",
            json!({ "rules": [{ "when": [], "then": "Bring" }] }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a verdict the domain cannot read is a rule that matches nothing while looking configured: {refused}"
    );
    assert!(
        refused["error"]["field_errors"]["rules"].is_string(),
        "{refused}"
    );

    let stored = darkhold_db::imports::intake_policy(harness.db(), "ao3")
        .await
        .expect("read policy");
    assert_eq!(
        stored.as_deref(),
        Some("{}"),
        "and nothing was stored: the refusal is not advisory"
    );
}

#[tokio::test]
async fn an_unknown_source_is_a_404_not_a_policy_for_nobody() {
    let harness = Harness::with_operator("unknown").await;
    let mut client = harness.signed_in().await;

    let (status, body) = client
        .put("/api/v1/admin/sources/nope/intake-policy", policy())
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}
