//! M45-43 step 4 — a bot can start the link flow itself, and learn the answer.
//!
//! §23.2's flow was owner-initiated: the owner minted a code in a browser, handed it to a reader, the reader confirmed on the site. That works for "my
//! bot, my account" and cannot work for a guild bot, which has a bearer credential rather than a session — so `darkbot` grew two invented endpoints
//! (`bot/link/start`, `bot/link/exchange`) to fill the gap, one of which minted a token the server never agreed to.
//!
//! This suite proves the gap is closed on the server's terms:
//!
//! 1. **A bot mints a code for a person.** `POST /me/bots/{id}/link-challenge` takes the bot's own bearer credential and names the platform user the
//!    code is for. No session involved.
//! 2. **The reader still confirms on the site.** The bot cannot confirm its own challenge, so "what am I granting" stays a page a person reads.
//! 3. **The bot learns who linked.** `GET /me/bots/{id}/links` answers with the pseud and the platform user, which is the question the code asked.
//! 4. **Neither door is a way to read somebody else's links.** A second bot's credential is refused with 404 rather than an explanation.

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server::{self, set_trust_proxy};
use darkhold_app::state::AppState;
use darkhold_db::DatabaseConfig;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tower::ServiceExt;

const PASSWORD: &str = "test-pass-1234";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-m45-43-{tag}-{}-{:?}",
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
    config.rate_limits.auth = darkhold_app::limiter::Quota {
        burst: 200,
        per_minute: 2_000,
    };
    config.rate_limits.write = darkhold_app::limiter::Quota {
        burst: 200,
        per_minute: 2_000,
    };
    config
}

struct Client {
    app: axum::Router,
    cookies: Vec<(String, String)>,
    bearer: Option<String>,
}

impl Client {
    fn new(app: axum::Router) -> Self {
        Self {
            app,
            cookies: Vec::new(),
            bearer: None,
        }
    }

    fn with_bearer(app: axum::Router, token: &str) -> Self {
        Self {
            app,
            cookies: Vec::new(),
            bearer: Some(token.to_owned()),
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
        if let Some(token) = &self.bearer {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        if !self.cookies.is_empty() {
            let joined = self
                .cookies
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("; ");
            builder = builder.header(header::COOKIE, joined);
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
            Some(value) => Body::from(value.to_string()),
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

    fn bot_client(&self, token: &str) -> Client {
        Client::with_bearer(
            server::build_router(AppState::new(self.config.clone(), self.tdb.db().clone())),
            token,
        )
    }

    /// A registered reader, with the client that holds its session.
    async fn reader(&self, handle: &str) -> Client {
        let mut client = self.client();
        let (status, body) = client
            .post(
                "/api/v1/auth/register",
                json!({
                    "email": format!("{handle}@example.test"),
                    "password": PASSWORD,
                    "handle": handle,
                    "display_name": handle,
                    "age_band": "adult",
                }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{handle}: {body}");
        client
    }

    /// Register a bot as the owner's session, and return `(bot_id, raw_token)`.
    async fn register_bot(&self, owner: &mut Client, name: &str) -> (String, String) {
        let (status, body) = owner
            .post(
                "/api/v1/me/bots",
                json!({
                    "name": name,
                    "contact": "ops@example.test",
                    "user_agent": "m45-43-test/1",
                    "scopes": ["content.read", "library.read"],
                }),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "register bot: {body}");
        (
            body["bot_id"].as_str().expect("bot_id").to_owned(),
            body["token"].as_str().expect("token").to_owned(),
        )
    }
}

#[tokio::test]
async fn a_bot_mints_a_code_for_a_platform_user_and_learns_who_linked() {
    let harness = Harness::new("bearer").await;
    let mut owner = harness.reader("owner").await;
    let (bot_id, token) = harness.register_bot(&mut owner, "Archivist").await;

    // 1. The bot starts the flow itself — no session, its own credential.
    let mut bot = harness.bot_client(&token);
    let (status, challenge) = bot
        .post(
            &format!("/api/v1/me/bots/{bot_id}/link-challenge"),
            json!({
                "scopes": ["library.read"],
                "platform": "discord",
                "platform_user_id": "987654321",
            }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a bot must be able to mint its own challenge: {status} {challenge}"
    );
    let code = challenge["code"].as_str().expect("code").to_owned();

    // 2. The reader confirms on the site — the bot cannot do this, which is the point.
    let mut reader = harness.reader("kim").await;
    let (status, described) = reader.get(&format!("/api/v1/bots/link/{code}")).await;
    assert_eq!(status, StatusCode::OK, "describe: {described}");
    assert_eq!(described["bot"], json!("Archivist"), "{described}");

    let (status, confirmed) = reader
        .post(&format!("/api/v1/bots/link/{code}"), json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "confirm: {status} {confirmed}");

    // 3. The bot asks which of its links is that person.
    let (status, links) = bot.get(&format!("/api/v1/me/bots/{bot_id}/links")).await;
    assert_eq!(status, StatusCode::OK, "links: {status} {links}");
    let rows = links["links"].as_array().expect("links array");
    assert_eq!(rows.len(), 1, "one link: {links}");
    assert_eq!(rows[0]["platform"], json!("discord"), "{links}");
    assert_eq!(rows[0]["platform_user"], json!("987654321"), "{links}");
    assert_eq!(
        rows[0]["pseud"],
        json!("kim"),
        "the bot learns the pseud the reader granted, not their account: {links}"
    );
}

#[tokio::test]
async fn a_bot_cannot_read_another_bots_links_or_mint_its_challenges() {
    let harness = Harness::new("not-yours").await;
    let mut owner = harness.reader("owner").await;
    let (first_id, first_token) = harness.register_bot(&mut owner, "First").await;
    let (second_id, second_token) = harness.register_bot(&mut owner, "Second").await;

    let mut first = harness.bot_client(&first_token);

    // Its own: allowed.
    let (status, _) = first
        .get(&format!("/api/v1/me/bots/{first_id}/links"))
        .await;
    assert_eq!(status, StatusCode::OK, "own links");

    // The other's: 404, not 403 — a bot learning that another bot exists changes nothing it can act on.
    let (status, refused) = first
        .get(&format!("/api/v1/me/bots/{second_id}/links"))
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "another bot's links: {status} {refused}"
    );

    let (status, refused) = first
        .post(
            &format!("/api/v1/me/bots/{second_id}/link-challenge"),
            json!({ "scopes": ["library.read"] }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "minting a challenge for another bot: {status} {refused}"
    );

    // And the second bot's own credential still works, so the refusal is about the pair and not about bot credentials in general.
    let mut second = harness.bot_client(&second_token);
    let (status, ok) = second
        .post(
            &format!("/api/v1/me/bots/{second_id}/link-challenge"),
            json!({ "scopes": ["library.read"] }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{ok}");
}

#[tokio::test]
async fn a_suspended_bot_mints_nothing() {
    let harness = Harness::new("suspended").await;
    let mut owner = harness.reader("owner").await;
    let (bot_id, token) = harness.register_bot(&mut owner, "Sleepy").await;

    // Suspend through the operator door's store function: the state column is what the two doors check.
    darkhold_db::external::set_bot_state(harness.tdb.db(), &bot_id, "suspended")
        .await
        .expect("suspend");

    let mut bot = harness.bot_client(&token);
    let (status, refused) = bot
        .post(
            &format!("/api/v1/me/bots/{bot_id}/link-challenge"),
            json!({ "scopes": ["library.read"] }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a suspended bot gets nothing: {status} {refused}"
    );

    let (status, refused) = bot.get(&format!("/api/v1/me/bots/{bot_id}/links")).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "and reads nothing: {status} {refused}"
    );
}
