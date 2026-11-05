//! M45-50 — the admin surface for scheduled gravity.
//!
//! What this file can prove from outside is the refusal: the three routes sit behind the admin pseud,
//! and a registered reader is not it — and a refusal writes nothing. The window rule is proven below
//! the routes, twice: five tests in `darkhold_domain::theme_window` (half-open, offset required,
//! backwards refused) and five in `darkhold-db/tests/theme_schedule.rs` (boundaries, overlap, revoke,
//! instance scoping).
//!
//! The happy path is not reachable from a test, and that is a fact about the route rather than a gap
//! here: the gate compares against a hard-coded `ADMIN_PSEUD_ID`, so only the seeded operator pseud
//! passes. Making it reachable would mean either seeding that exact pseud and forging its session, or
//! turning the constant into configuration — the second is a row of its own, and the first would test
//! a path no real caller takes.

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::Database;
use serde_json::Value;
use test_support::TestDb;
use tower::ServiceExt;

const GOOD_PASSWORD: &str = "a-long-enough-passphrase";

struct Harness {
    _tdb: TestDb,
    _dir: std::path::PathBuf,
    config: Config,
    db: Database,
}

impl Harness {
    async fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "darkhold-m63-06-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        let mut config = Config::development_defaults();
        config.storage.root = dir.clone();
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
        Client {
            app: server::build_router(AppState::new(self.config.clone(), self.db.clone())),
            cookies: Vec::new(),
        }
    }
}

struct Client {
    app: axum::Router,
    cookies: Vec<(String, String)>,
}

impl Client {
    async fn request(
        &mut self,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder().method(method).uri(uri);
        let cookies = self
            .cookies
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; ");
        if !cookies.is_empty() {
            builder = builder.header(header::COOKIE, cookies);
        }
        if !matches!(method, "GET" | "HEAD" | "OPTIONS") {
            if let Some((_, token)) = self.cookies.iter().find(|(k, _)| k == "darkhold_csrf") {
                builder = builder.header("x-csrf-token", token.clone());
            }
        }
        let request = match body {
            Some(ref value) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(value).expect("serialise")))
                .expect("request"),
            None => builder.body(Body::empty()).expect("request"),
        };
        let response = self.app.clone().oneshot(request).await.expect("response");
        let status = response.status();
        for value in response.headers().get_all(header::SET_COOKIE) {
            let Ok(raw) = value.to_str() else { continue };
            let pair = raw.split(';').next().unwrap_or(raw);
            let Some((name, val)) = pair.split_once('=') else {
                continue;
            };
            self.cookies.retain(|(key, _)| key != name);
            if !val.is_empty() {
                self.cookies.push((name.to_owned(), val.to_owned()));
            }
        }
        let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
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
}

async fn reader(harness: &Harness, handle: &str) -> Client {
    let mut client = harness.client();
    let (status, body) = client
        .post(
            "/api/v1/auth/register",
            serde_json::json!({
                "email": format!("{handle}@example.com"),
                "password": GOOD_PASSWORD,
                "handle": handle,
                "display_name": handle,
                "age_band": "adult",
            }),
        )
        .await;
    assert!(status.is_success(), "register {handle}: {status} {body}");
    client
}

#[tokio::test]
async fn a_reader_cannot_schedule_the_instance_s_gravity() {
    let harness = Harness::new("refused").await;
    let mut client = reader(&harness, "reader").await;

    let (status, body) = client.get("/federation/theme/schedule").await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "only the operator pseud may read the schedule list: {body}"
    );

    let (status, body) = client
        .post(
            "/federation/theme/schedule",
            serde_json::json!({
                "label": "Christmas",
                "theme_vector": { "christmas": 5.0 },
                "starts_at": "2026-12-24T00:00:00Z",
                "ends_at": "2026-12-26T00:00:00Z",
            }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a seasonal vector changes what every reader is shown: it is an operator's decision: {body}"
    );

    let (status, body) = client
        .request("DELETE", "/federation/theme/schedule/any-id", None)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    // And nothing was written: the refusal is a refusal, not a partial write.
    let listed =
        darkhold_db::theme_schedule::list_for_instance(&harness.db, &harness.config.site.base_url)
            .await
            .expect("list");
    assert!(
        listed.is_empty(),
        "a refused request left {} row(s) behind",
        listed.len()
    );
}
