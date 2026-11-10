//! The Vanguard selection method had no door (M70-13).
//!
//! §16.18 says an instance picks its Taste Vanguards by one of three methods: `resonance_threshold`, `admin_appointment`, or
//! `contribution_volume`. The first needs an admin taste profile that nothing computes; the second is one account at a time and
//! has had its door since M18. The third is a query — `roles::select_by_contribution_volume`, "top N curators by bookmark and
//! review count in the last 90 days" — and nothing called it or `select_vanguards`, the dispatcher written to apply it.
//!
//! So an operator who wanted the top curators had to find them by hand. The fix is a door that runs the selection, and the shape
//! of the door is a decision: **it is a dry run unless the operator says `apply`.** A Vanguard can pin works to the front page,
//! so a selection that granted the role as a side effect of asking "who would qualify?" would make a question into an appointment.
//! Contribution volume is a measurement of activity; whether that deserves the pin is the operator's call, and the door is built
//! so the call is theirs.

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
        "darkhold-vanguard-selection-{tag}-{}-{:?}",
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
    // `vanguard` is opt-in by design (registry.rs), so the gate answers 404 MODULE_UNAVAILABLE on an instance that has not
    // enabled it — which is what this test hit before the line below existed. The door is behind the module like every other
    // switchable surface; a test that skipped the gate would be testing a route no deployed instance serves.
    config.modules.enabled.push("vanguard".to_owned());
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

    /// Register a reader and return its account id.
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

    /// `count` bookmarks, so the selection has something to count. `subject_id` is a foreign-key-free UUID column, so a random
    /// one is a valid bookmark target on both engines.
    async fn bookmark(&self, account: &str, count: usize) {
        for _ in 0..count {
            let id = uuid::Uuid::new_v4().to_string();
            let subject = uuid::Uuid::new_v4().to_string();
            let now = darkhold_db::identity::now_rfc3339();
            let sql = self.db.sql(
                "INSERT INTO bookmarks (id, account_id, subject_type, subject_id, position_permille, note, created_at, updated_at)
                 VALUES (?, ?, 'work', ?, NULL, '', ?, ?)",
                "INSERT INTO bookmarks (id, account_id, subject_type, subject_id, position_permille, note, created_at, updated_at)
                 VALUES (?::uuid, ?::uuid, 'work', ?::uuid, NULL, '', ?, ?)",
            );
            // Both arms end in `;` so the match is a statement: the two pools are different types, and returning the query
            // result unifies them into an error the compiler reports against the arms rather than the call.
            match self.db.backend() {
                darkhold_db::Backend::Sqlite => {
                    sqlx::query(&sql)
                        .bind(&id)
                        .bind(account)
                        .bind(&subject)
                        .bind(&now)
                        .bind(&now)
                        .execute(self.db.sqlite_pool().expect("sqlite"))
                        .await
                        .expect("bookmark");
                }
                darkhold_db::Backend::Postgres => {
                    sqlx::query(&sql)
                        .bind(&id)
                        .bind(account)
                        .bind(&subject)
                        .bind(&now)
                        .bind(&now)
                        .execute(self.db.postgres_pool().expect("postgres"))
                        .await
                        .expect("bookmark");
                }
            }
        }
    }
}

async fn vanguards(db: &darkhold_db::Database) -> Vec<serde_json::Value> {
    darkhold_db::roles::list_vanguards(db)
        .await
        .expect("list vanguards")
}

#[tokio::test]
async fn the_contribution_volume_selection_is_a_dry_run_until_it_is_applied() {
    let h = Harness::new("vanguard-selection").await;
    let (mut operator, operator_account) = h.reader("operator").await;
    test_support::set_account_trust(&h.db, &operator_account, 5).await;

    let (_, busy) = h.reader("busy-curator").await;
    let (_, idle) = h.reader("idle-reader").await;
    h.bookmark(&busy, 3).await;
    h.bookmark(&idle, 1).await;

    // 1. A dry run answers with the account the measurement picked, and grants nothing. This is the assertion that separates the
    // door from an appointment: asking who would qualify must not make them Vanguards.
    let (status, body) = operator
        .send(
            "POST",
            "/api/v1/vanguards/select",
            Some(serde_json::json!({ "method": "contribution_volume", "limit": 1 })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["applied"], false, "a dry run is the default: {body}");
    let candidates = body["candidates"].as_array().expect("candidates");
    assert_eq!(candidates.len(), 1, "limit is honoured: {body}");
    assert_eq!(
        candidates[0], busy,
        "the busiest curator is the candidate: {body}"
    );
    assert!(
        vanguards(&h.db).await.is_empty(),
        "a dry run must not grant the role"
    );

    // 2. Applied, the same selection grants it — with the method recorded, because a Vanguard row that does not say how it was
    // chosen cannot be re-examined when the method changes.
    let (status, body) = operator
        .send(
            "POST",
            "/api/v1/vanguards/select",
            Some(serde_json::json!({ "method": "contribution_volume", "limit": 1, "apply": true })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["applied"], true, "{body}");
    let listed = vanguards(&h.db).await;
    assert_eq!(listed.len(), 1, "one candidate, one grant: {listed:?}");
    assert_eq!(listed[0]["account_id"], busy);
    assert_eq!(listed[0]["method"], "contribution_volume");

    // 3. A method this door cannot compute refuses by name. Returning an empty list would answer "nobody qualifies" to a question
    // it never asked — `resonance_threshold` needs a taste profile nothing builds, and `admin_appointment` is one account at a
    // time through the grant door that exists.
    let (status, body) = operator
        .send(
            "POST",
            "/api/v1/vanguards/select",
            Some(serde_json::json!({ "method": "resonance_threshold" })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body.to_string().contains("method"),
        "the refusal names the field: {body}"
    );

    // 4. Operator-only: a reader gets the same answer as a stranger would get from a door that does not exist for them.
    let (mut reader, _) = h.reader("curious-reader").await;
    let (status, _) = reader
        .send(
            "POST",
            "/api/v1/vanguards/select",
            Some(serde_json::json!({ "method": "contribution_volume" })),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
