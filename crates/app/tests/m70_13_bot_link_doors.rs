//! The bot link flow over HTTP (§23.2, M54-02) — the four doors `external.rs` was missing.
//!
//! Five rules, each only real if a request proves it:
//!
//! * a challenge is minted by the **bot's own account**, and somebody else's bot id is a `404`;
//! * a code discloses what is being asked and nothing else — no pseud, no token, no account;
//! * a confirmation may only **narrow** the requested scopes; adding one is refused, because a disclosure page that can be
//!   bypassed is decoration;
//! * the token is returned once, expires, and acts as the pseud that confirmed it;
//! * a suspended bot's token stops working — the `state` column had a writer and no reader until now.

use std::path::PathBuf;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use serde_json::{json, Value};
use test_support::TestDb;
use tower::ServiceExt;

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-m7013bot-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

struct Client {
    app: Router,
    cookies: Vec<(String, String)>,
}

impl Client {
    fn new(app: Router) -> Self {
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

    fn rejoin(&self, app: Router) -> Self {
        Self {
            app,
            cookies: self.cookies.clone(),
        }
    }

    async fn send(&mut self, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut builder = Request::builder()
            .method(Method::from_bytes(method.as_bytes()).expect("method"))
            .uri(uri);
        if body.is_some() {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
        }
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
        let body = body.map_or_else(Body::empty, |value| Body::from(value.to_string()));
        let response = self
            .app
            .clone()
            .oneshot(builder.body(body).expect("request"))
            .await
            .expect("response");
        let status = response.status();
        self.capture(&response);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// A request carrying a bearer token instead of the session cookie — the credential a bot actually uses.
    async fn send_with_token(
        &mut self,
        method: &str,
        uri: &str,
        token: &str,
    ) -> (StatusCode, Value) {
        let request = Request::builder()
            .method(Method::from_bytes(method.as_bytes()).expect("method"))
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

    async fn register(&mut self, handle: &str) -> String {
        let (status, body) = self
            .send(
                "POST",
                "/api/v1/auth/register",
                Some(json!({
                    "email": format!("m7013bot-{handle}@example.test"),
                    "password": "test-pass-1234",
                    "handle": handle,
                    "display_name": handle,
                    "age_band": "adult",
                })),
            )
            .await;
        assert!(
            status == StatusCode::CREATED || status == StatusCode::OK,
            "register {handle}: {body}"
        );
        let (_, me) = self.send("GET", "/api/v1/auth/me", None).await;
        me["account"]["id"]
            .as_str()
            .or_else(|| me["id"].as_str())
            .expect("account id")
            .to_owned()
    }
}

fn config(dir: &std::path::Path, operator: Option<String>) -> Config {
    let mut config = Config::development_defaults();
    config.storage.root = dir.to_path_buf();
    if let Some(id) = operator {
        config.administration.operator_account_id = Some(id.parse().expect("account id"));
    }
    config
}

async fn register_bot(client: &mut Client, name: &str) -> String {
    let (status, body) = client
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
    assert_eq!(status, StatusCode::OK, "{body}");
    body["bot_id"].as_str().expect("a bot id").to_owned()
}

#[tokio::test]
async fn a_challenge_is_minted_by_the_owner_and_confirmed_by_a_reader() {
    let dir = scratch_dir("link");
    let tdb = TestDb::connect_with_dir("m7013bot", &dir).await;

    let mut owner = Client::new(server::build_router(AppState::new(
        config(&dir, None),
        tdb.db().clone(),
    )));
    let owner_account = owner.register("bot-owner").await;
    let bot = register_bot(&mut owner, "Archivist").await;

    // Minting: the owner asks for scopes, and the code comes back with a lifetime.
    let (status, body) = owner
        .send(
            "POST",
            &format!("/api/v1/me/bots/{bot}/link-challenge"),
            Some(json!({ "scopes": ["content.read", "library.read"] })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["expires_in_seconds"], json!(600), "{body}");
    let code = body["code"].as_str().expect("a code").to_owned();
    assert!(code.len() >= 6, "a code a person can type: {body}");

    // A scope this build does not know is refused at minting, not at the confirmation page.
    let (status, body) = owner
        .send(
            "POST",
            &format!("/api/v1/me/bots/{bot}/link-challenge"),
            Some(json!({ "scopes": ["admin.everything"] })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    // A stranger cannot mint a code for somebody else's bot.
    let mut stranger = Client::new(server::build_router(AppState::new(
        config(&dir, None),
        tdb.db().clone(),
    )));
    stranger.register("bot-stranger").await;
    let (status, body) = stranger
        .send(
            "POST",
            &format!("/api/v1/me/bots/{bot}/link-challenge"),
            Some(json!({ "scopes": ["content.read"] })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // The reader who will grant it: the code discloses the request and nothing else.
    let mut reader = Client::new(server::build_router(AppState::new(
        config(&dir, None),
        tdb.db().clone(),
    )));
    reader.register("bot-reader").await;
    let (status, body) = reader
        .send("GET", &format!("/api/v1/bots/link/{code}"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["state"], json!("pending"), "{body}");
    assert_eq!(body["bot_id"], json!(bot), "{body}");
    assert_eq!(
        body["requested_scopes"],
        json!(["content.read", "library.read"]),
        "the request is shown in full: {body}"
    );
    for forbidden in ["pseud_id", "token", "token_id", "account_id"] {
        assert!(
            body.get(forbidden).is_none(),
            "a code discloses the request, not the identity: {forbidden} in {body}"
        );
    }

    // The reader narrows what it grants; the grant is what the token carries.
    let (status, body) = reader
        .send(
            "POST",
            &format!("/api/v1/bots/link/{code}"),
            Some(json!({ "scopes": ["content.read"] })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["linked"], json!(true), "{body}");
    assert_eq!(body["scopes"], json!(["content.read"]), "{body}");
    let secret = body["token"].as_str().expect("a token, once").to_owned();

    // The token works, as the pseud that confirmed it.
    let (status, body) = reader
        .send_with_token("GET", "/api/v1/me/credential", &secret)
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a fresh bot token authenticates: {body}"
    );

    // The code is spent: a second confirmation is refused, and the challenge now reads `used`.
    let (status, body) = reader
        .send(
            "POST",
            &format!("/api/v1/bots/link/{code}"),
            Some(json!({ "scopes": ["content.read"] })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let (_, body) = reader
        .send("GET", &format!("/api/v1/bots/link/{code}"), None)
        .await;
    assert_eq!(body["state"], json!("used"), "{body}");

    // An unknown code is a 404 for both doors.
    let (status, _) = reader
        .send("GET", "/api/v1/bots/link/nosuchcode", None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Suspension: the operator stops the bot, and its token stops working — the check `bot_for_token` was written for.
    let mut operator = owner.rejoin(server::build_router(AppState::new(
        config(&dir, Some(owner_account)),
        tdb.db().clone(),
    )));
    let (status, body) = operator
        .send(
            "POST",
            &format!("/api/v1/admin/bots/{bot}/state"),
            Some(json!({ "state": "suspended" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = reader
        .send_with_token("GET", "/api/v1/me/credential", &secret)
        .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "a suspended bot's token is not a credential: {body}"
    );
}

#[tokio::test]
async fn a_confirmation_may_narrow_the_request_but_never_widen_it() {
    let dir = scratch_dir("narrow");
    let tdb = TestDb::connect_with_dir("m7013botnarrow", &dir).await;

    let mut owner = Client::new(server::build_router(AppState::new(
        config(&dir, None),
        tdb.db().clone(),
    )));
    owner.register("narrow-owner").await;
    let bot = register_bot(&mut owner, "Narrow").await;

    let (_, body) = owner
        .send(
            "POST",
            &format!("/api/v1/me/bots/{bot}/link-challenge"),
            Some(json!({ "scopes": ["content.read"] })),
        )
        .await;
    let code = body["code"].as_str().expect("a code").to_owned();

    let mut reader = Client::new(server::build_router(AppState::new(
        config(&dir, None),
        tdb.db().clone(),
    )));
    reader.register("narrow-reader").await;

    // Adding a scope the bot did not ask for is refused: a disclosure page that can be bypassed is decoration.
    let (status, body) = reader
        .send(
            "POST",
            &format!("/api/v1/bots/link/{code}"),
            // `library.read` is a real scope and the bot did not ask for it: the widening is a widening, not a typo the
            // scope parser would have refused on its own.
            Some(json!({ "scopes": ["content.read", "library.read"] })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    // Granting nothing is refused too: a linked bot holding a token it cannot use is a worse answer than a refusal.
    let (status, body) = reader
        .send(
            "POST",
            &format!("/api/v1/bots/link/{code}"),
            Some(json!({ "scopes": [] })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    // Both refusals left the challenge pending, so the reader can still do the right thing.
    let (_, body) = reader
        .send("GET", &format!("/api/v1/bots/link/{code}"), None)
        .await;
    assert_eq!(body["state"], json!("pending"), "{body}");
}
