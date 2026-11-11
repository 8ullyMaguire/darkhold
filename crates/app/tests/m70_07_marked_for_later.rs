//! M70-07 — `Marked for Later` over real HTTP.
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
    // No `config.database`: the router is built over `TestDb`'s database, which honours `DARKHOLD_TEST_PG_URL`. A sqlite
    // URL left here is a second, unread configuration that once made this file look SQLite-only to the dialect gate.
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
        // `account_id` is a `uuid` column on PostgreSQL and TEXT on SQLite: the column is cast to text so the bind is text on
        // both engines and the comparison means the same thing in both.
        let pseud = test_support::scalar_pair_with(
            db,
            "SELECT id FROM pseuds WHERE account_id = ?",
            "SELECT id::text FROM pseuds WHERE account_id = $1::uuid",
            account_id,
        )
        .await
        .expect("the registered account has a pseud");
        let work = uuid::Uuid::new_v4().to_string();
        // Two texts rather than one: the ids land in `uuid` columns on PostgreSQL, where a text bind is not coerced by a
        // placeholder the way a literal is. `?` and `$n::uuid` say the same thing in the two dialects.
        test_support::exec_binds(
            db,
            "INSERT INTO works (id, owner_pseud_id, title, summary, language, rating, visibility, lifecycle, \
             completion, created_at, updated_at, version) \
             VALUES (?, ?, 'The Long Way Round', '', 'en', 'general', 'public', 'published', 'complete', \
                     '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1)",
            "INSERT INTO works (id, owner_pseud_id, title, summary, language, rating, visibility, lifecycle, \
             completion, created_at, updated_at, version) \
             VALUES ($1::uuid, $2::uuid, 'The Long Way Round', '', 'en', 'general', 'public', 'published', 'complete', \
                     '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1)",
            &[&work, &pseud],
        )
        .await;
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
        // GS-3 (2026-11-11) put the CSRF guard on the whole `api` router: a write must echo the token the session cookie set, or it is a
        // bare 403 that reads like a permissions bug. `test_support`'s client has done this since it existed; hand-rolled clients drift.
        if method != "GET" && method != "HEAD" {
            if let Some((_, token)) = self.cookies.iter().find(|(k, _)| k == "darkhold_csrf") {
                builder = builder.header("x-csrf-token", token.clone());
            }
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
async fn the_queue_is_private_idempotent_and_scoped_to_its_owner() {
    let h = Harness::new("mfl").await;
    let mut reader = h.client();
    let account = reader.register("reader@mfl.test", "hollowgast").await;
    let work = h.seed_work(&account).await;

    // No session, no queue.
    let mut anon = h.client();
    let (status, _) = anon
        .post(&format!("/api/v1/works/{work}/marked-for-later"), json!({}))
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = anon
        .request("GET", "/api/v1/me/marked-for-later", None)
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Queueing twice reports the second as already there rather than duplicating.
    let (status, body) = reader
        .post(&format!("/api/v1/works/{work}/marked-for-later"), json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["marked"], true);
    assert_eq!(body["added"], true);
    let (status, body) = reader
        .post(&format!("/api/v1/works/{work}/marked-for-later"), json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["added"], false, "the second ask adds nothing");

    let (status, body) = reader
        .request("GET", "/api/v1/me/marked-for-later", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["count"], 1);
    assert_eq!(body["items"][0]["work_id"], work);

    // Another signed-in reader's queue is empty: the read is scoped, not filtered.
    let mut other = h.client();
    other.register("other@mfl.test", "bystander").await;
    let (status, body) = other
        .request("GET", "/api/v1/me/marked-for-later", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["count"], 0);

    // A work that does not exist is a 404, not a row pointing at nothing.
    let (status, _) = reader
        .post(
            "/api/v1/works/01957900-0000-4000-8000-00000000dead/marked-for-later",
            json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Dropping is idempotent in the same way.
    let (status, body) = reader
        .request(
            "DELETE",
            &format!("/api/v1/works/{work}/marked-for-later"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["removed"], true);
    let (status, body) = reader
        .request(
            "DELETE",
            &format!("/api/v1/works/{work}/marked-for-later"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["removed"], false);
    let (_, body) = reader
        .request("GET", "/api/v1/me/marked-for-later", None)
        .await;
    assert_eq!(body["count"], 0);
}
