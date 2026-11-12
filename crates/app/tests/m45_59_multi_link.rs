//! M45-59 — a bot in a guild has **many** readers, so a link is per person, not per bot.
//!
//! §23.2's confirmation used to rotate the *registration's* credential to the token it had just issued. That reads fine for
//! a personal bot and is wrong for the bot M45-43 is about: the second member of a server to link silently revoked the
//! first one's credential, and because `bot_for_token` looked the registration up by the token it was created with, it could
//! not see any link at all afterwards.
//!
//! Three properties, one per assertion group:
//!
//! 1. **Links coexist.** Two people link to one bot; both credentials act as the pseud that granted them, and the
//!    registration's own credential is untouched — a link is not a re-registration.
//! 2. **A re-link replaces one person's credential and nobody else's.** The boundary is (bot, pseud), not (bot).
//! 3. **Suspension reaches links.** The operator stops the bot once and every linked reader stops with it, which is only
//!    true because `bot_for_token` now resolves a link token to the registration.

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
        "darkhold-m45-59-{tag}-{}-{:?}",
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

    /// Take another client's cookies: the operator door is the *same* session opened on a router that names this account
    /// as the operator, which is how the configuration reaches `require_operator` without a second sign-in.
    fn adopt(&mut self, other: &Self) {
        self.cookies = other.cookies.clone();
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

    /// A request carrying a bearer token instead of the session cookie — the credential a bot actually uses.
    async fn send_with_token(&mut self, uri: &str, token: &str) -> (StatusCode, Value) {
        let request = Request::builder()
            .method("GET")
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::empty())
            .expect("request");
        let response = self.app.clone().oneshot(request).await.expect("response");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn register(&mut self, tag: &str, handle: &str) -> String {
        let (status, body) = self
            .send(
                "POST",
                "/api/v1/auth/register",
                Some(json!({
                    "email": format!("{tag}@example.test"),
                    "password": PASSWORD,
                    "handle": handle,
                    "display_name": handle,
                    "age_band": "adult",
                })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "register {handle}: {body}");
        let (status, me) = self.send("GET", "/api/v1/auth/me", None).await;
        assert_eq!(status, StatusCode::OK, "{me}");
        me["account"]["id"].as_str().expect("account id").to_owned()
    }

    /// Register a bot for the signed-in account: (bot id, the registration's own token).
    async fn register_bot(&mut self, name: &str) -> (String, String) {
        let (status, body) = self
            .send(
                "POST",
                "/api/v1/me/bots",
                Some(json!({
                    "name": name,
                    "contact": "ops@example.test",
                    "user_agent": format!("{name}/1.0 (+https://example.test)"),
                    "scopes": ["content.read"],
                })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "register bot: {body}");
        (
            body["bot_id"].as_str().expect("a bot id").to_owned(),
            body["token"]
                .as_str()
                .expect("the registration token")
                .to_owned(),
        )
    }

    /// The owner mints a challenge for its own bot and the signed-in reader confirms it, granting `content.read`.
    async fn link(&mut self, bot: &str) -> String {
        let (status, body) = self
            .send(
                "POST",
                &format!("/api/v1/me/bots/{bot}/link-challenge"),
                Some(json!({ "scopes": ["content.read"] })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "mint a challenge: {body}");
        let code = body["code"].as_str().expect("a code").to_owned();

        let (status, body) = self
            .send(
                "POST",
                &format!("/api/v1/bots/link/{code}"),
                Some(json!({ "scopes": ["content.read"] })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "confirm the link: {body}");
        body["token"].as_str().expect("a token, once").to_owned()
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

    /// The same instance with one account as the operator — the door `set_bot_state` guards.
    fn config_as_operator(&self, account: &str) -> Config {
        let mut config = self.config.clone();
        config.administration.operator_account_id = Some(account.parse().expect("account id"));
        config
    }

    /// The credential id the registration names. `rebind_bot_token` used to rewrite it on every confirmation.
    async fn registration_token_id(&self, bot: &str) -> Option<String> {
        use sqlx::Row;
        match self.db().backend() {
            darkhold_db::Backend::Sqlite => {
                sqlx::query("SELECT token_id FROM bot_registrations WHERE id = ?1")
                    .bind(bot)
                    .fetch_optional(self.db().sqlite_pool().expect("sqlite"))
                    .await
                    .expect("read the registration")
                    .and_then(|row| row.get::<Option<String>, _>("token_id"))
            }
            darkhold_db::Backend::Postgres => {
                sqlx::query("SELECT token_id FROM bot_registrations WHERE id = $1::text")
                    .bind(bot)
                    .fetch_optional(self.db().postgres_pool().expect("postgres"))
                    .await
                    .expect("read the registration")
                    .and_then(|row| row.get::<Option<String>, _>("token_id"))
            }
        }
    }
}

#[tokio::test]
async fn two_readers_link_to_one_bot_and_both_credentials_work() {
    let harness = Harness::new("coexist").await;

    let mut owner = harness.client();
    owner.register("coexist-owner", "Owner").await;
    let (bot, registration_token) = owner.register_bot("Archivist").await;
    let registration_before = harness
        .registration_token_id(&bot)
        .await
        .expect("the registration names its credential");

    // The owner mints for a second reader: minting is owner-only, confirming is the reader's own act.
    let mut second = harness.client();
    second.register("coexist-second", "Second").await;
    let second_token = {
        let code = owner
            .send(
                "POST",
                &format!("/api/v1/me/bots/{bot}/link-challenge"),
                Some(json!({ "scopes": ["content.read"] })),
            )
            .await;
        assert_eq!(code.0, StatusCode::OK, "{}", code.1);
        let code = code.1["code"].as_str().expect("a code").to_owned();
        let (status, body) = second
            .send(
                "POST",
                &format!("/api/v1/bots/link/{code}"),
                Some(json!({ "scopes": ["content.read"] })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body["token"].as_str().expect("a token").to_owned()
    };

    // Then the owner links itself.
    let owner_token = owner.link(&bot).await;

    for (who, token) in [
        ("the first reader", &second_token),
        ("the owner", &owner_token),
    ] {
        let (status, body) = owner.send_with_token("/api/v1/me/credential", token).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "{who}'s credential still works after the other linked — this is the regression the row exists for: {body}"
        );
    }

    // A link is not a re-registration: the registration still names the credential it was created with. The old code
    // rotated this column on every confirmation, which is exactly how the first reader's credential died.
    let after = harness.registration_token_id(&bot).await;
    assert_eq!(
        after.as_deref(),
        Some(registration_before.as_str()),
        "bot_registrations.token_id moved on link: {registration_before} -> {after:?}"
    );
    let _ = &registration_token;

    // And each link acts as the person who granted it, not as the bot's owner.
    let (status, body) = owner
        .send_with_token("/api/v1/me/credential", &second_token)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let acting = body["pseud_id"]
        .as_str()
        .or_else(|| body["credential"]["acting_pseud_id"].as_str())
        .unwrap_or_default()
        .to_owned();
    let (_, mine) = owner
        .send_with_token("/api/v1/me/credential", &owner_token)
        .await;
    let owner_acting = mine["pseud_id"]
        .as_str()
        .or_else(|| mine["credential"]["acting_pseud_id"].as_str())
        .unwrap_or_default()
        .to_owned();
    assert!(
        !acting.is_empty() && acting != owner_acting,
        "two readers, two acting pseuds: second={acting:?} owner={owner_acting:?}"
    );

    let _ = harness.db();
}

#[tokio::test]
async fn a_relink_replaces_one_person_and_leaves_the_other_readers_alone() {
    let harness = Harness::new("relink").await;

    let mut owner = harness.client();
    owner.register("relink-owner", "Owner").await;
    let (bot, _registration) = owner.register_bot("Archivist").await;

    let mut reader = harness.client();
    reader.register("relink-reader", "Reader").await;
    let first = {
        let (_, body) = owner
            .send(
                "POST",
                &format!("/api/v1/me/bots/{bot}/link-challenge"),
                Some(json!({ "scopes": ["content.read"] })),
            )
            .await;
        let code = body["code"].as_str().expect("a code").to_owned();
        let (status, body) = reader
            .send(
                "POST",
                &format!("/api/v1/bots/link/{code}"),
                Some(json!({ "scopes": ["content.read"] })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body["token"].as_str().expect("a token").to_owned()
    };

    // The owner links too, then links *again*: only its own previous credential is replaced.
    let owner_first = owner.link(&bot).await;
    let owner_second = owner.link(&bot).await;

    let (status, body) = owner
        .send_with_token("/api/v1/me/credential", &owner_second)
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the newest credential works: {body}"
    );
    let (status, body) = owner
        .send_with_token("/api/v1/me/credential", &owner_first)
        .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "re-linking replaces that person's previous credential: {body}"
    );
    let (status, body) = owner.send_with_token("/api/v1/me/credential", &first).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "and nobody else's — the boundary is (bot, pseud), not (bot): {body}"
    );
}

#[tokio::test]
async fn suspending_the_bot_stops_every_linked_reader() {
    let harness = Harness::new("suspend").await;

    let mut owner = harness.client();
    let owner_account = owner.register("suspend-owner", "Owner").await;
    let (bot, registration_token) = owner.register_bot("Archivist").await;

    let mut reader = harness.client();
    reader.register("suspend-reader", "Reader").await;
    let reader_token = {
        let (_, body) = owner
            .send(
                "POST",
                &format!("/api/v1/me/bots/{bot}/link-challenge"),
                Some(json!({ "scopes": ["content.read"] })),
            )
            .await;
        let code = body["code"].as_str().expect("a code").to_owned();
        let (status, body) = reader
            .send(
                "POST",
                &format!("/api/v1/bots/link/{code}"),
                Some(json!({ "scopes": ["content.read"] })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body["token"].as_str().expect("a token").to_owned()
    };

    let mut operator = Client::new(server::build_router(AppState::new(
        harness.config_as_operator(&owner_account),
        harness.db().clone(),
    )));
    operator.adopt(&owner);
    let (status, body) = operator
        .send(
            "POST",
            &format!("/api/v1/admin/bots/{bot}/state"),
            Some(json!({ "state": "suspended" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    for (who, token) in [
        ("a linked reader", &reader_token),
        ("the bot itself", &registration_token),
    ] {
        let (status, body) = owner.send_with_token("/api/v1/me/credential", token).await;
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "{who} is stopped by one operator action: {body}"
        );
    }
}
