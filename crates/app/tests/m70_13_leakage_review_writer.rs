//! The leakage view had no writer (M70-13).
//!
//! §52.1 is a rule about what an operator can *say* about the instance's taste: each reviewed artifact gets a disposition, and
//! the view refuses to certify completeness. The view exists — `GET /admin/discovery/leakage` reads
//! `taste_leakage::review_rows` — and the store's writer `record_review` had no caller anywhere in `crates/app`. So the queue
//! was permanently empty: an operator could read it, and could not produce a single row for it.
//!
//! That is the shape this ratchet keeps finding — a moderation queue with no producer is worse than no queue, because it reads
//! as "nothing leaks here" when it means "nobody has ever looked". The door added here is the producer, and this file is its
//! proof from both ends: the write answers, and the row the view then serves is the row that was written.

use std::path::Path;

use axum::http::{header, Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use test_support::TestDb;
use tower::ServiceExt;

const PASSWORD: &str = "a-long-enough-passphrase";

fn scratch_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-leakage-writer-{tag}-{}-{:?}",
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
    fn new(app: axum::Router) -> Self {
        Self {
            app,
            cookies: Vec::new(),
        }
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

    async fn send(
        &mut self,
        method: &str,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> (StatusCode, serde_json::Value) {
        let mut builder = Request::builder().method(method).uri(path);
        if !self.cookies.is_empty() {
            let header_value = self
                .cookies
                .iter()
                .map(|(n, v)| format!("{n}={v}"))
                .collect::<Vec<_>>()
                .join("; ");
            builder = builder.header(header::COOKIE, header_value);
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
        let request = match body {
            Some(body) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(body.to_string()))
                .expect("request"),
            None => builder.body(axum::body::Body::empty()).expect("request"),
        };
        let response = self.app.clone().oneshot(request).await.expect("response");
        let status = response.status();
        self.capture(&response);
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("body");
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }
}

struct Harness {
    _tdb: TestDb,
    _dir: std::path::PathBuf,
    config: Config,
    db: darkhold_db::Database,
}

impl Harness {
    async fn new(tag: &str) -> Self {
        let dir = scratch_dir(tag);
        let config = config_for(&dir);
        let tdb = TestDb::connect_with_dir(tag, &dir).await;
        let db = tdb.db().clone();
        Self {
            _tdb: tdb,
            _dir: dir,
            config,
            db,
        }
    }

    fn client(&self) -> Client {
        Client::new(server::build_router(AppState::new(
            self.config.clone(),
            self.db.clone(),
        )))
    }

    /// A client whose config names `account` as the operator.
    ///
    /// `admin_discovery::require_operator` compares the session's account against
    /// `config.administration.operator_account_id` — a configured *identity*, not a trust level — and refuses anyone else with
    /// the same 404 an unknown route gives (`taste_leakage_view.rs` documents that choice). So the harness has to register the
    /// account first, name it in the config, and then build the router: the first version of this file granted the account
    /// trust 5, which is the *vanguard* doors' predicate, and got a 404 that read like a missing route.
    fn operator_client(&self, account: &str) -> Client {
        let mut config = self.config.clone();
        config.administration.operator_account_id = Some(account.parse().expect("an account id"));
        Client::new(server::build_router(AppState::new(config, self.db.clone())))
    }

    /// Sign a fresh client in, capturing the session cookies it is issued.
    async fn sign_in(&self, client: &mut Client, email: &str) {
        let (status, body) = client
            .send(
                "POST",
                "/api/v1/auth/login",
                Some(serde_json::json!({ "email": email, "password": PASSWORD })),
            )
            .await;
        assert!(status.is_success(), "login {email}: {status} {body}");
    }

    async fn reader(&self, handle: &str) -> (Client, String) {
        let mut client = self.client();
        let (status, body) = client
            .send(
                "POST",
                "/api/v1/auth/register",
                Some(serde_json::json!({
                    "email": format!("{handle}@example.com"),
                    "password": PASSWORD,
                    "handle": handle,
                    "display_name": handle,
                    "age_band": "adult",
                })),
            )
            .await;
        assert!(status.is_success(), "register {handle}: {status} {body}");
        let (status, me) = client.send("GET", "/api/v1/auth/me", None).await;
        assert_eq!(status, StatusCode::OK, "{me}");
        let account = me["account"]["id"].as_str().expect("account id").to_owned();
        (client, account)
    }
}

/// Every key in a JSON document, at any depth — used to assert what a response does *not* say.
fn keys(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, nested) in map {
                out.push(key.clone());
                keys(nested, out);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                keys(item, out);
            }
        }
        _ => {}
    }
}

#[tokio::test]
async fn an_operator_can_produce_the_row_the_view_serves() {
    let h = Harness::new("leakage-writer").await;
    // Register through the default config, then name that account as the operator and sign a fresh client in.
    let (_bootstrap, operator_account) = h.reader("operator").await;
    let mut operator = h.operator_client(&operator_account);
    h.sign_in(&mut operator, "operator@example.com").await;

    // 1. The queue starts empty, and that is exactly why a producer was needed: an empty queue with no writer reads as
    // "nothing leaks" when it means "nobody has looked".
    let (status, body) = operator
        .send("GET", "/api/v1/admin/discovery/leakage", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["rows"].as_array().expect("rows").len(), 0, "{body}");

    // 2. Write one. The disposition is the operator's judgement, so it is the operator's to send.
    let (status, written) = operator
        .send(
            "POST",
            "/api/v1/admin/discovery/leakage",
            Some(serde_json::json!({
                "artifact": "the 'similar to what you shelved' strip on a work page",
                "inferable": "that the operator has read this author before",
                "ease": "derived",
                "disposition": "coarsen",
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{written}");
    assert!(
        written["id"].as_str().is_some_and(|id| !id.is_empty()),
        "{written}"
    );

    // 3. The view serves it — the writer and the reader are one table, so a row that is written is a row that is shown.
    let (status, body) = operator
        .send("GET", "/api/v1/admin/discovery/leakage", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let rows = body["rows"].as_array().expect("rows");
    assert_eq!(rows.len(), 1, "{body}");
    assert_eq!(rows[0]["disposition"], "coarsen");
    assert_eq!(rows[0]["ease"], "derived");
    assert!(rows[0]["artifact"]
        .as_str()
        .is_some_and(|a| a.contains("similar to what you shelved")));

    // 4. §52.1: the view must not report how much it has reviewed. A count that moves with a configuration change is itself a
    // probe, so no key in the response may be one.
    let mut all_keys = Vec::new();
    keys(&body, &mut all_keys);
    for forbidden in ["count", "total", "reviewed", "n"] {
        assert!(
            !all_keys.iter().any(|k| k == forbidden),
            "the leakage view must not publish a count ({forbidden}): {body}"
        );
    }
    assert_eq!(body["exhaustive"], false);

    // 5. A disposition the schema does not know is refused by name, and nothing is written: a typo must not become a row the
    // ledger has to explain later.
    let (status, refused) = operator
        .send(
            "POST",
            "/api/v1/admin/discovery/leakage",
            Some(serde_json::json!({
                "artifact": "another artifact",
                "inferable": "something",
                "ease": "plain",
                "disposition": "tolerate",
            })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert!(
        refused.to_string().contains("disposition"),
        "the refusal names the field: {refused}"
    );
    let (_, body) = operator
        .send("GET", "/api/v1/admin/discovery/leakage", None)
        .await;
    assert_eq!(body["rows"].as_array().expect("rows").len(), 1, "{body}");

    // 6. Operator-only — and the refusal is **404**, not 403. `require_estate_hidden` (ADR 0003) refuses an operator door with
    // the same answer an unknown route gives, because "403" is itself the disclosure: it says the door exists and you may not
    // use it. This assertion was written as 403 and the door corrected it, which is the right direction for the test to be wrong.
    let (mut reader, _) = h.reader("curious-reader").await;
    let (status, _) = reader
        .send(
            "POST",
            "/api/v1/admin/discovery/leakage",
            Some(serde_json::json!({
                "artifact": "a",
                "inferable": "b",
                "ease": "plain",
                "disposition": "keep",
            })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
