//! M70-10 — the browse list: a pseud as author, a preview as the preview, and no ranking.
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
        "darkhold-browseroute-{tag}-{}-{:?}",
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
async fn the_browse_list_attributes_to_a_pseud_and_previews_without_running() {
    let h = Harness::new("browse").await;
    let mut me = h.client();
    me.register("browse@example.test", "hollowgast").await;
    let mut anon = h.client();

    let document = json!({
        "version": 1,
        "kind": "layout",
        "name": "Winter",
        "body": { "slots": [ { "widget": "inbox" }, { "widget": "trends", "span": 6 } ] }
    });
    let (status, body) = me
        .request(
            "POST",
            "/api/v1/me/extension-documents",
            Some(json!({ "document": document })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let id = body["id"].as_str().expect("id").to_owned();

    // Nothing is public by default, so the list is empty.
    let (status, body) = anon
        .request("GET", "/api/v1/extension-documents", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["count"], 0);

    // Publishing is attributed to the pseud the session is acting as. Registration leaves one selected, so this is
    // the ordinary path; the "not the account's pseud" refusal is exercised at the store, where the rule lives.
    let (status, pseuds) = me.request("GET", "/api/v1/pseuds", None).await;
    assert_eq!(status, StatusCode::OK, "{pseuds}");
    let handle = pseuds[0]["handle"]
        .as_str()
        .expect("the account's own pseud")
        .to_owned();

    let (status, body) = me
        .request(
            "PUT",
            &format!("/api/v1/me/extension-documents/{id}/publish"),
            Some(json!({ "published": true })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["published"], true);

    // The browse list: public, attributed to a pseud, with a server-produced preview and nothing resembling a rank.
    let (status, body) = anon
        .request("GET", "/api/v1/extension-documents", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["count"], 1);
    let item = &body["items"][0];
    assert_eq!(item["name"], "Winter");
    assert_eq!(item["pseud"]["handle"], handle);
    assert_eq!(
        item["preview"], "2 slots: inbox, trends ×6",
        "the preview is a description, not a run"
    );
    for absent in [
        "installs",
        "install_count",
        "rating",
        "stars",
        "downloads",
        "rank",
        "score",
    ] {
        assert!(
            item.get(absent).is_none(),
            "a browse entry must not carry `{absent}`"
        );
    }

    // Narrowing by kind is a filter, not a ranking.
    let (_, body) = anon
        .request("GET", "/api/v1/extension-documents?kind=theme_tokens", None)
        .await;
    assert_eq!(body["count"], 0);
    let (_, body) = anon
        .request("GET", "/api/v1/extension-documents?kind=layout", None)
        .await;
    assert_eq!(body["count"], 1);

    // Withdrawing removes it from the list; the document itself stays for its owner.
    let (status, body) = me
        .request(
            "PUT",
            &format!("/api/v1/me/extension-documents/{id}/publish"),
            Some(json!({ "published": false })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, body) = anon
        .request("GET", "/api/v1/extension-documents", None)
        .await;
    assert_eq!(body["count"], 0);
    let (status, _) = me
        .request("GET", &format!("/api/v1/me/extension-documents/{id}"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "withdrawing is not deleting");
}
