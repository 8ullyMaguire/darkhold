//! M70-01 — `POST /works/:id/orphan` over real HTTP.
//!
//! Four doors, and each asserts a different decision:
//!
//! * **The owner can orphan**, and the answer names the frozen author line rather than saying "ok".
//! * **A stranger cannot**, and learns nothing: the refusal is the same one a missing work gets, so the route
//!   cannot be used to enumerate work ids.
//! * **Doing it twice is refused**, because the second call would append a suffix and imply a second owner.
//! * **Without a session it is 401**, not 403 — there is nothing to be permitted yet.

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server::{self, set_trust_proxy};
use darkhold_app::state::AppState;
use darkhold_db::DatabaseConfig;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tower::ServiceExt;

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-orphanroute-{tag}-{}-{:?}",
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

struct Harness {
    _dir: PathBuf,
    tdb: test_support::TestDb,
    config: Config,
}

impl Harness {
    async fn new(tag: &str) -> Self {
        set_trust_proxy(false);
        let dir = scratch_dir(tag);
        let tdb = test_support::TestDb::connect_with_dir(tag, &dir).await;
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

    /// Seed a published work owned by the pseud of `account_id`, and return its id.
    async fn seed_work(&self, account_id: &str) -> String {
        let db = self.tdb.db();
        let pseud: String = sqlx::query_scalar("SELECT id FROM pseuds WHERE account_id = ?")
            .bind(account_id)
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("the registered account has a pseud");
        let work = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO works (id, owner_pseud_id, title, summary, language, rating, visibility, lifecycle, \
             completion, created_at, updated_at, version) \
             VALUES (?, ?, 'The Long Way Round', '', 'en', 'general', 'public', 'published', 'complete', \
                     '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1)",
        )
        .bind(&work)
        .bind(&pseud)
        .execute(db.sqlite_pool().expect("sqlite"))
        .await
        .expect("seed work");
        work
    }
}

const PASSWORD: &str = "test-pass-1234";

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
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json");
        for (name, value) in &self.cookies {
            builder = builder.header(header::COOKIE, format!("{name}={value}"));
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
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&body).into_owned()));
        (status, json)
    }

    async fn post(&mut self, uri: &str, body: Value) -> (StatusCode, Value) {
        self.request("POST", uri, Some(body)).await
    }

    async fn register(&mut self, email: &str, handle: &str) -> String {
        let (status, body) = self
            .post(
                "/api/v1/auth/register",
                json!({
                    "email": email,
                    "password": PASSWORD,
                    "handle": handle,
                    "display_name": handle,
                    "age_band": "adult",
                }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "register {handle}: {body}");
        let (status, me) = self.request("GET", "/api/v1/auth/me", None).await;
        assert_eq!(status, StatusCode::OK, "{me}");
        me["account"]["id"].as_str().expect("account id").to_owned()
    }
}

#[tokio::test]
async fn the_owner_orphans_and_a_stranger_learns_nothing() {
    let h = Harness::new("owner").await;
    let mut owner = h.client();
    let account = owner.register("author@orphan.test", "hollowgast").await;
    let work = h.seed_work(&account).await;

    // A stranger, signed in, asking about somebody else's work.
    let mut stranger = h.client();
    stranger.register("stranger@orphan.test", "bystander").await;
    let (status, body) = stranger
        .post(&format!("/api/v1/works/{work}/orphan"), json!({}))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("no work with that id"),
        "the refusal must be the same one a missing work gets: {body}"
    );

    // Anonymous: no session, nothing to permit.
    let mut anon = h.client();
    let (status, _) = anon
        .post(&format!("/api/v1/works/{work}/orphan"), json!({}))
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // The owner, and the answer names the author line it froze.
    let (status, body) = owner
        .post(&format!("/api/v1/works/{work}/orphan"), json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["orphaned"], true);
    assert_eq!(body["author_label"], "hollowgast (orphaned)");

    // Twice is refused, and refused with the *stranger's* answer rather than a special one: once orphaned the work
    // belongs to nobody, so the person who was its author is now no different from anyone else asking. Saying
    // "already orphaned" would confirm the id exists and that this caller used to own it.
    //
    // The store's `AlreadyAnOrphan` refusal is still reachable — `orphan_work` is public and `orphan_works_of` calls
    // it in a loop — so the rule is tested there (crates/db/tests/orphan_works.rs), not through this door.
    let (status, body) = owner
        .post(&format!("/api/v1/works/{work}/orphan"), json!({}))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("no work with that id"),
        "the second attempt must not reveal that the work exists: {body}"
    );
}
