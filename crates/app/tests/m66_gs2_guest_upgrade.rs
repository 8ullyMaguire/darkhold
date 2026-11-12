//! M66 GS-2 — what a guest did stays theirs when they register.
//!
//! The requirement is about **bookmarks and reading progress**, which are the two things a person accumulates before they
//! trust a site enough to give it an address. So the test does not assert "an upgrade happened" and call it done: it writes
//! the two artifacts as the guest, registers, and then reads the rows back.
//!
//! The design decision this pins: registration from a guest session **upgrades the account in place**. It does not create a
//! second account and does not fold one into the other, because `bookmarks.account_id` and `reading_progress.account_id`
//! are the key — leave the id alone and the rows cannot be lost by a partial fold. (§14.2's fold stays what it is for:
//! merging two *distinct* identities.)
//!
//! Four properties:
//!
//! 1. **Same account, same pseud.** The id in `/auth/me` after registering is the id before it, and the pseud is renamed —
//!    not replaced, which would orphan `reading_progress.pseud_id`.
//! 2. **The artifacts are still there**, one row each, owned by that account.
//! 3. **The guest address is gone** and `is_guest` is false: the synthetic `guest+…@guest.invalid` must not survive as
//!    somebody's login identifier.
//! 4. **Registering twice is refused** — with `session` named, not `email`, because the email was fine.

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
        "darkhold-m66-gs2-{tag}-{}-{:?}",
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
        Ok(url) if !url.is_empty() => DatabaseConfig::new(url),
        _ => DatabaseConfig::new(format!(
            "sqlite://{}/darkhold.sqlite?mode=rwc",
            dir.display()
        )),
    };
    config
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

    async fn send(&mut self, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json");
        for (name, value) in &self.cookies {
            builder = builder.header(header::COOKIE, format!("{name}={value}"));
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

    async fn post(&mut self, uri: &str, body: Value) -> (StatusCode, Value) {
        self.send("POST", uri, Some(body)).await
    }

    async fn get(&mut self, uri: &str) -> (StatusCode, Value) {
        self.send("GET", uri, None).await
    }
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

    fn db(&self) -> &darkhold_db::Database {
        self.tdb.db()
    }

    /// What a guest accumulates before registering: one bookmark and one reading position.
    async fn guest_artifacts(&self, account: &str, pseud: &str) {
        let now = "2026-11-11T00:00:00Z";
        match self.db().backend() {
            darkhold_db::Backend::Sqlite => {
                sqlx::query(
                    "INSERT INTO bookmarks (id, account_id, subject_type, subject_id, note, is_public, created_at, updated_at, version)
                     VALUES ('bm-1', ?1, 'work', 'w-1', 'come back to this', 0, ?2, ?2, 1)",
                )
                .bind(account)
                .bind(now)
                .execute(self.db().sqlite_pool().expect("sqlite"))
                .await
                .expect("bookmark as the guest");
                sqlx::query(
                    "INSERT INTO reading_progress (id, account_id, pseud_id, subject_type, subject_id, position_permille, created_at, updated_at, version)
                     VALUES ('rp-1', ?1, ?2, 'work', 'w-1', 420, ?3, ?3, 1)",
                )
                .bind(account)
                .bind(pseud)
                .bind(now)
                .execute(self.db().sqlite_pool().expect("sqlite"))
                .await
                .expect("progress as the guest");
            }
            darkhold_db::Backend::Postgres => {
                sqlx::query(
                    "INSERT INTO bookmarks (id, account_id, subject_type, subject_id, note, is_public, created_at, updated_at, version)
                     VALUES ('00000000-0000-4000-8000-0000000000b1'::uuid, $1::uuid, 'work', '00000000-0000-4000-8000-0000000000c1'::uuid, 'come back to this', FALSE, $2, $2, 1)",
                )
                .bind(account)
                .bind(now)
                .execute(self.db().postgres_pool().expect("postgres"))
                .await
                .expect("bookmark as the guest");
                sqlx::query(
                    "INSERT INTO reading_progress (id, account_id, pseud_id, subject_type, subject_id, position_permille, created_at, updated_at, version)
                     VALUES ('00000000-0000-4000-8000-0000000000a1'::uuid, $1::uuid, $2::uuid, 'work', '00000000-0000-4000-8000-0000000000c1'::uuid, 420, $3, $3, 1)",
                )
                .bind(account)
                .bind(pseud)
                .bind(now)
                .execute(self.db().postgres_pool().expect("postgres"))
                .await
                .expect("progress as the guest");
            }
        }
    }

    async fn owned_rows(&self, account: &str) -> (i64, i64) {
        use sqlx::Row;
        let (bookmarks, progress) = match self.db().backend() {
            darkhold_db::Backend::Sqlite => {
                let b = sqlx::query("SELECT COUNT(*) AS n FROM bookmarks WHERE account_id = ?1")
                    .bind(account)
                    .fetch_one(self.db().sqlite_pool().expect("sqlite"))
                    .await
                    .expect("count bookmarks")
                    .get::<i64, _>("n");
                let p =
                    sqlx::query("SELECT COUNT(*) AS n FROM reading_progress WHERE account_id = ?1")
                        .bind(account)
                        .fetch_one(self.db().sqlite_pool().expect("sqlite"))
                        .await
                        .expect("count progress")
                        .get::<i64, _>("n");
                (b, p)
            }
            darkhold_db::Backend::Postgres => {
                let b =
                    sqlx::query("SELECT COUNT(*) AS n FROM bookmarks WHERE account_id = $1::uuid")
                        .bind(account)
                        .fetch_one(self.db().postgres_pool().expect("postgres"))
                        .await
                        .expect("count bookmarks")
                        .get::<i64, _>("n");
                let p = sqlx::query(
                    "SELECT COUNT(*) AS n FROM reading_progress WHERE account_id = $1::uuid",
                )
                .bind(account)
                .fetch_one(self.db().postgres_pool().expect("postgres"))
                .await
                .expect("count progress")
                .get::<i64, _>("n");
                (b, p)
            }
        };
        (bookmarks, progress)
    }

    async fn email_of(&self, account: &str) -> String {
        use sqlx::Row;
        match self.db().backend() {
            darkhold_db::Backend::Sqlite => sqlx::query("SELECT email FROM accounts WHERE id = ?1")
                .bind(account)
                .fetch_one(self.db().sqlite_pool().expect("sqlite"))
                .await
                .expect("email")
                .get::<String, _>("email"),
            darkhold_db::Backend::Postgres => {
                sqlx::query("SELECT email FROM accounts WHERE id = $1::uuid")
                    .bind(account)
                    .fetch_one(self.db().postgres_pool().expect("postgres"))
                    .await
                    .expect("email")
                    .get::<String, _>("email")
            }
        }
    }
}

#[tokio::test]
async fn a_guest_registers_and_keeps_everything_they_did() {
    let harness = Harness::new("keep").await;
    let version = harness.config.consent.version;
    let mut reader = harness.client();

    let (status, guest) = reader
        .post(
            "/api/v1/auth/guest",
            json!({ "consent": { "version": version } }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{guest}");
    let guest_id = guest["account"]["id"]
        .as_str()
        .expect("guest id")
        .to_owned();

    let (_, me) = reader.get("/api/v1/auth/me").await;
    let pseud_id = me["pseuds"][0]["id"]
        .as_str()
        .expect("a guest pseud")
        .to_owned();
    assert!(
        me["pseuds"][0]["handle"]
            .as_str()
            .unwrap_or_default()
            .starts_with("guest-"),
        "the guest handle is a placeholder nobody has seen: {me}"
    );

    harness.guest_artifacts(&guest_id, &pseud_id).await;
    assert_eq!(
        harness.owned_rows(&guest_id).await,
        (1, 1),
        "the guest really has both"
    );

    // Register — from the guest session, which is the whole point.
    let (status, registered) = reader
        .post(
            "/api/v1/auth/register",
            json!({
                "email": "kept@example.test",
                "password": PASSWORD,
                "handle": "Kept",
                "display_name": "Kept",
                "age_band": "adult",
            }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "an upgrade is 200, not 201: the account already existed: {registered}"
    );
    assert_eq!(
        registered["account"]["id"],
        json!(guest_id),
        "the same account, upgraded in place: {registered}"
    );
    assert_eq!(
        registered["account"]["is_guest"],
        json!(false),
        "{registered}"
    );

    // The artifacts did not move, because nothing moved.
    assert_eq!(
        harness.owned_rows(&guest_id).await,
        (1, 1),
        "the bookmark and the reading position are still owned by the account"
    );

    // The pseud is renamed, not replaced: `reading_progress.pseud_id` names it.
    let (_, me) = reader.get("/api/v1/auth/me").await;
    assert_eq!(
        me["pseuds"][0]["id"],
        json!(pseud_id),
        "renaming keeps the pseud id, which is what progress is keyed on: {me}"
    );
    assert_eq!(me["pseuds"][0]["handle"], json!("Kept"), "{me}");
    assert_eq!(me["account"]["is_guest"], json!(false), "{me}");

    // And the synthetic guest address is gone, rather than left as somebody's login identifier.
    let email = harness.email_of(&guest_id).await;
    assert_eq!(
        email, "kept@example.test",
        "the guest address must not survive"
    );
    assert!(
        !email.contains("guest.invalid"),
        "a registered account must not keep the synthetic guest address: {email}"
    );

    // The new credential works, so the reader can come back tomorrow.
    let mut later = harness.client();
    let (status, body) = later
        .post(
            "/api/v1/auth/login",
            json!({ "email": "kept@example.test", "password": PASSWORD }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["account"]["id"], json!(guest_id), "{body}");
}

#[tokio::test]
async fn registering_twice_is_refused_by_name_and_changes_nothing() {
    let harness = Harness::new("twice").await;
    let version = harness.config.consent.version;
    let mut reader = harness.client();

    let (_, guest) = reader
        .post(
            "/api/v1/auth/guest",
            json!({ "consent": { "version": version } }),
        )
        .await;
    let guest_id = guest["account"]["id"].as_str().expect("id").to_owned();

    let body = json!({
        "email": "once@example.test",
        "password": PASSWORD,
        "handle": "Once",
        "display_name": "Once",
        "age_band": "adult",
    });
    let (status, first) = reader.post("/api/v1/auth/register", body.clone()).await;
    assert_eq!(status, StatusCode::OK, "{first}");

    let (status, second) = reader
        .post(
            "/api/v1/auth/register",
            json!({
                "email": "twice@example.test",
                "password": PASSWORD,
                "handle": "Twice",
                "display_name": "Twice",
                "age_band": "adult",
            }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{second}");
    assert!(
        second["error"]["field_errors"].get("session").is_some(),
        "the refusal names the session — the email was fine: {second}"
    );

    // Nothing about the first registration changed.
    let email = harness.email_of(&guest_id).await;
    assert_eq!(
        email, "once@example.test",
        "the second attempt must not rewrite the account"
    );
    let (_, me) = reader.get("/api/v1/auth/me").await;
    assert_eq!(me["pseuds"][0]["handle"], json!("Once"), "{me}");
}
