//! A conversation is readable by its participants and by nobody else (M70-13).
//!
//! `community::is_participant` was written for exactly this question and had no caller anywhere in `crates/app`. Both message
//! doors — `GET` and `POST /conversations/{id}/messages` — took the conversation id from the path and never asked whether the
//! caller was in it:
//!
//! * `list_messages` filters by `conversation_id`, `deleted_at` and blocks, so **any** signed-in account that knew or guessed an
//!   id could read the whole thread;
//! * `post_message` read the participants — but only to check blocks — so anyone could write into somebody else's conversation.
//!
//! This is the most serious shape the ratchet has found: not a dead surface, a live one with no lock. The refusal must also not
//! be an oracle: a stranger's answer for a real conversation and for a made-up id has to be the same 404, because "403" would
//! confirm the thread exists and belongs to somebody.

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
        "darkhold-conversation-privacy-{tag}-{}-{:?}",
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
            if let Some((_, token)) = self.cookies.iter().find(|(name, _)| name == "darkhold_csrf") {
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
        (status, serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null))
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

#[tokio::test]
async fn a_conversation_is_readable_by_its_participants_and_nobody_else() {
    let h = Harness::new("conversation-privacy").await;
    let (mut alice, _alice_account) = h.reader("alice").await;
    let (mut bob, bob_account) = h.reader("bob").await;
    let (mut mallory, mallory_account) = h.reader("mallory").await;
    assert_ne!(bob_account, mallory_account);

    // 1. A conversation between alice and bob, and one message in it.
    let (status, created) = alice
        .send(
            "POST",
            "/api/v1/conversations",
            Some(serde_json::json!({ "participant": bob_account })),
        )
        .await;
    assert!(status.is_success(), "create conversation: {status} {created}");
    let conversation = created["id"].as_str().expect("conversation id").to_owned();

    let (status, sent) = alice
        .send(
            "POST",
            &format!("/api/v1/conversations/{conversation}/messages"),
            Some(serde_json::json!({ "body": "the plan is to meet at eight" })),
        )
        .await;
    assert!(status.is_success(), "send: {status} {sent}");

    // 2. Bob is in it and reads it.
    let (status, body) = bob
        .send("GET", &format!("/api/v1/conversations/{conversation}/messages"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["items"].as_array().expect("items").len(),
        1,
        "a participant sees the thread: {body}"
    );

    // 3. Mallory is not, and gets nothing — not the thread, and not a confirmation that it exists.
    let (status, refused) = mallory
        .send("GET", &format!("/api/v1/conversations/{conversation}/messages"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{refused}");

    // 4. Writing is refused the same way, and nothing lands: the thread still holds exactly one message.
    let (status, refused_post) = mallory
        .send(
            "POST",
            &format!("/api/v1/conversations/{conversation}/messages"),
            Some(serde_json::json!({ "body": "I am reading your mail" })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{refused_post}");
    let (_, body) = bob
        .send("GET", &format!("/api/v1/conversations/{conversation}/messages"), None)
        .await;
    assert_eq!(
        body["items"].as_array().expect("items").len(),
        1,
        "a refused write must not land: {body}"
    );

    // 5. No oracle: a stranger's answer for a real conversation is byte-identical to the answer for an id that was never one.
    let (status, invented) = mallory
        .send(
            "GET",
            "/api/v1/conversations/00000000-0000-4000-8000-000000000000/messages",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        invented["error"]["code"], refused["error"]["code"],
        "the refusal for a real thread must not differ from the one for an invented id: \
         {refused} vs {invented}"
    );
    assert_eq!(
        invented["error"]["message"], refused["error"]["message"],
        "same code is not enough: the message must not name the resource either"
    );
}
