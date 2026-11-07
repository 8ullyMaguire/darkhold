//! M16-13 — the theme model end to end: tokens, slots, scoping, preview, reset, safe mode.
//!
//! The domain has the arithmetic (contrast, the vocabulary, safe-mode precedence). This file proves the doors
//! cannot be talked around it: an unreadable palette is refused at save and again at publish, a reader cannot
//! rearrange the building, a preview is session-scoped and expires, reset deletes rather than writes, and safe
//! mode makes every theme — published or previewed — irrelevant until somebody says otherwise.

use std::path::PathBuf;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use serde_json::{json, Value};
use test_support::TestDb;
use tower::ServiceExt;

const PASSWORD: &str = "test-pass-1234";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-m1613-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

struct Harness {
    _dir: PathBuf,
    tdb: TestDb,
    config: Config,
}

impl Harness {
    async fn new(tag: &str) -> Self {
        server::set_trust_proxy(false);
        let dir = scratch_dir(tag);
        let tdb = TestDb::connect_with_dir(tag, &dir).await;
        let mut config = Config::development_defaults();
        config.storage.root = dir.clone();
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

    async fn register(&mut self, handle: &str) -> String {
        let (status, body) = self
            .send(
                "POST",
                "/api/v1/auth/register",
                Some(json!({
                    "email": format!("m16-13-{handle}@example.test"),
                    "password": PASSWORD,
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
        let (status, me) = self.send("GET", "/api/v1/auth/me", None).await;
        assert_eq!(status, StatusCode::OK, "{me}");
        me["account"]["id"]
            .as_str()
            .or_else(|| me["id"].as_str())
            .expect("account id")
            .to_owned()
    }
}

fn message(body: &Value) -> String {
    body["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

fn warm_palette() -> Value {
    json!({
        "--dh-surface": "#0f0f12",
        "--dh-ink": "#e8e6e0",
        "--dh-accent": "#c9a227",
        "--dh-accent-ink": "#12100a",
        "--dh-reading-surface": "#1a1a1f",
        "--dh-reading-ink": "#ece9e2",
    })
}

#[tokio::test]
async fn an_unreadable_theme_is_refused_at_save_and_the_refusal_names_the_pair() {
    let h = Harness::new("contrast").await;
    let mut reader = h.client();
    reader.register("theme-reader").await;

    let (status, body) = reader
        .send(
            "POST",
            "/api/v1/themes",
            Some(json!({
                "name": "unreadable",
                "palette": { "--dh-surface": "#121216", "--dh-ink": "#141418" },
            })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let text = message(&body);
    assert!(
        text.contains("--dh-ink"),
        "the message names the token: {text}"
    );
    assert!(text.contains(":1"), "and the measured ratio: {text}");

    // Nothing was stored: the list is empty, so a refused save is not a stored draft.
    let (status, body) = reader.send("GET", "/api/v1/themes", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["themes"], json!([]), "{body}");
}

#[tokio::test]
async fn a_good_theme_saves_as_a_draft_and_only_a_publish_makes_it_the_appearance() {
    let h = Harness::new("publish").await;
    let mut reader = h.client();
    reader.register("theme-reader").await;

    let (status, saved) = reader
        .send(
            "POST",
            "/api/v1/themes",
            Some(json!({"name": "warm", "palette": warm_palette()})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["state"], json!("draft"), "{saved}");
    assert!(saved["declarations"]
        .as_str()
        .unwrap_or_default()
        .contains("--dh-accent: #c9a227;"));

    // A draft is not an appearance.
    let (_, effective) = reader.send("GET", "/api/v1/themes/effective", None).await;
    assert_ne!(
        effective["source"],
        json!("reader"),
        "a saved draft must not render before it is published: {effective}"
    );

    let id = saved["id"].as_str().expect("id");
    let (status, published) = reader
        .send(
            "POST",
            &format!("/api/v1/themes/{id}/publish"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{published}");
    assert_eq!(published["published"], json!(true), "{published}");

    let (_, effective) = reader.send("GET", "/api/v1/themes/effective", None).await;
    assert_eq!(effective["source"], json!("reader"), "{effective}");
    assert_eq!(
        effective["palette"]["--dh-accent"],
        json!("#c9a227"),
        "{effective}"
    );
}

#[tokio::test]
async fn saving_a_published_theme_again_demotes_it() {
    let h = Harness::new("demote").await;
    let mut reader = h.client();
    reader.register("theme-reader").await;

    let (_, saved) = reader
        .send(
            "POST",
            "/api/v1/themes",
            Some(json!({"name": "warm", "palette": warm_palette()})),
        )
        .await;
    let id = saved["id"].as_str().expect("id").to_owned();
    reader
        .send(
            "POST",
            &format!("/api/v1/themes/{id}/publish"),
            Some(json!({})),
        )
        .await;
    assert_eq!(
        reader.send("GET", "/api/v1/themes/effective", None).await.1["source"],
        json!("reader")
    );

    // The bytes that were published are not the bytes just written, so a save takes it back to draft.
    let (status, body) = reader
        .send(
            "POST",
            "/api/v1/themes",
            Some(json!({"name": "warm", "palette": warm_palette(), "layout": {}})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["state"], json!("draft"), "{body}");
    assert_ne!(
        reader.send("GET", "/api/v1/themes/effective", None).await.1["source"],
        json!("reader"),
        "an edited theme is a draft until somebody publishes it again"
    );
}

#[tokio::test]
async fn a_reader_may_recolour_but_not_rearrange() {
    let h = Harness::new("layout").await;
    let mut reader = h.client();
    reader.register("theme-reader").await;

    let (status, body) = reader
        .send(
            "POST",
            "/api/v1/themes",
            Some(json!({"name": "rearranged", "palette": warm_palette(), "layout": {"nav": "aside"}})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(message(&body).contains("layout"), "{}", message(&body));

    // An operator may: the instance is the building.
    let account = reader.send("GET", "/api/v1/auth/me", None).await.1;
    let account_id = account["account"]["id"]
        .as_str()
        .or_else(|| account["id"].as_str())
        .expect("account id")
        .to_owned();
    let mut h = h;
    h.config.administration.operator_account_id = Some(account_id.parse().expect("account id"));
    let mut operator = h.client();
    operator.cookies = reader.cookies.clone();

    let (status, body) = operator
        .send(
            "POST",
            "/api/v1/themes",
            Some(json!({
                "scope": "instance",
                "name": "archive",
                "palette": warm_palette(),
                "layout": {"nav": "aside", "aside": "stacked"},
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let id = body["id"].as_str().expect("id");

    let (status, published) = operator
        .send(
            "POST",
            &format!("/api/v1/themes/{id}/publish"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{published}");
    assert_eq!(
        published["layout"]["nav"],
        json!("aside"),
        "the placement is honoured"
    );
    assert_eq!(
        published["layout"]["header"],
        json!("stacked"),
        "regions the theme did not place keep their core placement: {published}"
    );

    // And a theme cannot be published by someone who merely knows its id.
    let mut stranger = h.client();
    stranger.register("stranger").await;
    let (status, body) = stranger
        .send(
            "POST",
            &format!("/api/v1/themes/{id}/publish"),
            Some(json!({})),
        )
        .await;
    assert!(
        status == StatusCode::UNPROCESSABLE_ENTITY || status == StatusCode::FORBIDDEN,
        "a stranger must not publish the instance theme: {status} {body}"
    );
}

#[tokio::test]
async fn a_preview_is_this_session_only_and_it_expires() {
    let h = Harness::new("preview").await;
    let mut author = h.client();
    author.register("preview-author").await;
    let (_, saved) = author
        .send(
            "POST",
            "/api/v1/themes",
            Some(json!({"name": "loud", "palette": {"--dh-accent": "#c9a227", "--dh-accent-ink": "#12100a"}})),
        )
        .await;
    let id = saved["id"].as_str().expect("id").to_owned();

    let (status, body) = author
        .send(
            "POST",
            &format!("/api/v1/themes/{id}/preview"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["preview"], json!(true), "{body}");

    let (_, effective) = author.send("GET", "/api/v1/themes/effective", None).await;
    assert_eq!(
        effective["source"],
        json!("preview"),
        "the previewing session sees the preview: {effective}"
    );

    // Somebody else, signed in on the same instance, sees nothing of it.
    let mut other = h.client();
    other.register("other-reader").await;
    let (_, theirs) = other.send("GET", "/api/v1/themes/effective", None).await;
    assert_ne!(
        theirs["source"],
        json!("preview"),
        "a preview is not a publication: {theirs}"
    );

    // Expiry is enforced server-side: push the row into the past and the preview is gone.
    sqlx::query("UPDATE theme_previews SET expires_at = '2000-01-01T00:00:00Z' WHERE theme_id = ?")
        .bind(&id)
        .execute(h.tdb.db().sqlite_pool().expect("sqlite"))
        .await
        .expect("expire preview");
    let (_, after) = author.send("GET", "/api/v1/themes/effective", None).await;
    assert_ne!(
        after["source"],
        json!("preview"),
        "an expired preview is not a preview: {after}"
    );
}

#[tokio::test]
async fn reset_deletes_the_published_theme_and_safe_mode_overrides_everything() {
    let mut h = Harness::new("reset").await;
    let mut reader = h.client();
    let account = reader.register("theme-reader").await;

    let (_, saved) = reader
        .send(
            "POST",
            "/api/v1/themes",
            Some(json!({"name": "warm", "palette": warm_palette()})),
        )
        .await;
    let id = saved["id"].as_str().expect("id").to_owned();
    reader
        .send(
            "POST",
            &format!("/api/v1/themes/{id}/publish"),
            Some(json!({})),
        )
        .await;

    // Safe mode beats the published theme.
    h.config.administration.operator_account_id = Some(account.parse().expect("account id"));
    let mut operator = h.client();
    operator.cookies = reader.cookies.clone();

    let (status, body) = operator
        .send(
            "POST",
            "/api/v1/operator/themes/safe-mode",
            Some(json!({"active": true, "reason": "the last theme crashed the reader"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["effective"], json!("core"), "{body}");

    let (_, effective) = operator.send("GET", "/api/v1/themes/effective", None).await;
    assert_eq!(effective["source"], json!("core"), "{effective}");
    assert_eq!(effective["safe_mode"], json!(true), "{effective}");
    assert!(
        effective["safe_mode_reason"]
            .as_str()
            .unwrap_or_default()
            .contains("crashed"),
        "{effective}"
    );

    // Entering safe mode without a reason is refused: a state nobody can explain is a bug with no subject.
    let (status, body) = operator
        .send(
            "POST",
            "/api/v1/operator/themes/safe-mode",
            Some(json!({"active": true})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    // Leaving it brings the published theme back — the theme was never lost, only unconsulted.
    operator
        .send(
            "POST",
            "/api/v1/operator/themes/safe-mode",
            Some(json!({"active": false})),
        )
        .await;
    let (_, effective) = operator.send("GET", "/api/v1/themes/effective", None).await;
    assert_eq!(effective["source"], json!("reader"), "{effective}");

    // Reset is a delete: the theme is gone, not replaced by a copy of the defaults.
    let (status, body) = operator.send("DELETE", "/api/v1/themes", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["removed"], json!(1), "{body}");

    let (_, effective) = operator.send("GET", "/api/v1/themes/effective", None).await;
    assert_eq!(effective["source"], json!("core"), "{effective}");

    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM theme_profiles")
        .fetch_one(h.tdb.db().sqlite_pool().expect("sqlite"))
        .await
        .expect("count");
    assert_eq!(
        rows, 0,
        "reset deletes the row rather than writing the default into it"
    );
}
