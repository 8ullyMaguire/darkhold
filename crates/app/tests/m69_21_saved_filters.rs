//! M69-21 — the Saved Filter's public life over HTTP: bind a digit, follow a filter, pin a blend.
//!
//! These tests are about the doors, not the query engine: each one names a rule that is only real if the route enforces it —
//! the collision refusal, the private filter that cannot be followed, the switch that closes every door with a `404`, and the
//! digit a reader cannot press.

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
        "darkhold-m6922-{tag}-{}-{:?}",
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
                    "email": format!("m69-22-{handle}@example.test"),
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

fn router_with(dir: &std::path::Path, tdb: &TestDb, on: bool) -> axum::Router {
    let mut config = Config::development_defaults();
    config.storage.root = dir.to_path_buf();
    config.discovery.saved_filters = on;
    server::build_router(AppState::new(config, tdb.db().clone()))
}

async fn create_view(client: &mut Client, name: &str, scope: &str) -> String {
    let (status, body) = client
        .send(
            "POST",
            "/api/v1/saved-views",
            Some(json!({
                "name": name,
                "kind": "catalogue",
                "params": "sort=likes",
                "scope": scope,
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_str().expect("a view id").to_owned()
}

#[tokio::test]
async fn a_digit_opens_one_filter_and_a_collision_is_refused() {
    let dir = scratch_dir("saved-filters-keybind");
    let tdb = TestDb::connect_with_dir("m69-21-keybind", &dir).await;
    let mut reader = Client::new(router_with(&dir, &tdb, true));
    reader.register("digit-reader").await;

    let morning = create_view(&mut reader, "Morning", "public").await;
    let evening = create_view(&mut reader, "Evening", "public").await;

    let (status, body) = reader
        .send(
            "PATCH",
            &format!("/api/v1/saved-views/{morning}/keybind"),
            Some(json!({ "keybind": 4 })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["note"].as_str().unwrap_or_default().contains('4'),
        "the answer says which key: {body}"
    );

    let (status, body) = reader.send("GET", "/api/v1/me/keybinds", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["keybinds"][0]["key"], json!(4));

    // The reader chooses which filter keeps the key; the store does not swap them.
    let (status, body) = reader
        .send(
            "PATCH",
            &format!("/api/v1/saved-views/{evening}/keybind"),
            Some(json!({ "keybind": 4 })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains('4'),
        "the refusal names the digit: {body}"
    );

    // Zero and ten are not digits a reader presses for a filter.
    for digit in [0, 10] {
        let (status, body) = reader
            .send(
                "PATCH",
                &format!("/api/v1/saved-views/{evening}/keybind"),
                Some(json!({ "keybind": digit })),
            )
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{digit}: {body}");
    }

    // Clearing is a body of `null`, and the digit becomes available.
    let (status, _) = reader
        .send(
            "PATCH",
            &format!("/api/v1/saved-views/{morning}/keybind"),
            Some(json!({ "keybind": null })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = reader
        .send(
            "PATCH",
            &format!("/api/v1/saved-views/{evening}/keybind"),
            Some(json!({ "keybind": 4 })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "the freed digit is bindable");
}

#[tokio::test]
async fn a_stranger_cannot_bind_or_see_someone_elses_filter() {
    let dir = scratch_dir("saved-filters-stranger");
    let tdb = TestDb::connect_with_dir("m69-21-stranger", &dir).await;

    let mut owner = Client::new(router_with(&dir, &tdb, true));
    owner.register("filter-owner").await;
    let view = create_view(&mut owner, "Owner's filter", "public").await;

    let mut stranger = Client::new(router_with(&dir, &tdb, true));
    stranger.register("filter-stranger").await;
    let (status, body) = stranger
        .send(
            "PATCH",
            &format!("/api/v1/saved-views/{view}/keybind"),
            Some(json!({ "keybind": 2 })),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "403 would confirm somebody's private filter exists: {body}"
    );
}

#[tokio::test]
async fn a_public_filter_can_be_followed_and_a_private_one_cannot() {
    let dir = scratch_dir("saved-filters-follow");
    let tdb = TestDb::connect_with_dir("m69-21-follow", &dir).await;

    let mut owner = Client::new(router_with(&dir, &tdb, true));
    owner.register("follow-owner").await;
    let public = create_view(&mut owner, "Shared queue", "public").await;
    let private = create_view(&mut owner, "Only mine", "private").await;

    let mut reader = Client::new(router_with(&dir, &tdb, true));
    reader.register("follow-reader").await;

    let (status, body) = reader
        .send(
            "POST",
            &format!("/api/v1/saved-views/{public}/follow"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["following"], json!(true));
    assert_eq!(body["subscribers"], json!(1));

    let (status, body) = reader
        .send(
            "POST",
            &format!("/api/v1/saved-views/{public}/follow"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["already_followed"],
        json!(true),
        "following twice is following once: {body}"
    );
    assert_eq!(body["subscribers"], json!(1));

    let (status, body) = reader
        .send("GET", "/api/v1/me/followed-filters", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["followed"].as_array().map(Vec::len), Some(1));
    assert_eq!(body["followed"][0]["name"], json!("Shared queue"));

    // The owner's next edit of a private filter must not publish into a stranger's feed.
    let (status, body) = reader
        .send(
            "POST",
            &format!("/api/v1/saved-views/{private}/follow"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("private"),
        "{body}"
    );

    let (status, body) = reader
        .send(
            "DELETE",
            &format!("/api/v1/saved-views/{public}/follow"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["was_followed"], json!(true));
    assert_eq!(body["subscribers"], json!(0));
    let (status, body) = reader
        .send("GET", "/api/v1/me/followed-filters", None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["followed"].as_array().map(Vec::len), Some(0));
}

#[tokio::test]
async fn a_blend_is_created_answered_and_deleted() {
    let dir = scratch_dir("saved-filters-blend");
    let tdb = TestDb::connect_with_dir("m69-21-blend", &dir).await;
    let mut reader = Client::new(router_with(&dir, &tdb, true));
    reader.register("blend-reader").await;
    let left = create_view(&mut reader, "Unfinished", "public").await;
    let right = create_view(&mut reader, "Short", "public").await;

    let (status, body) = reader
        .send(
            "POST",
            "/api/v1/saved-view-blends",
            Some(json!({
                "name": "Queue and shorts",
                "left": left,
                "right": right,
                "mode": "interleave",
                "pinned": true,
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let id = body["blend"]["id"].as_str().expect("a blend id").to_owned();

    let (status, body) = reader.send("GET", "/api/v1/saved-view-blends", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["blends"].as_array().map(Vec::len), Some(1));

    let (status, body) = reader
        .send(
            "GET",
            &format!("/api/v1/saved-view-blends/{id}/items"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["blend"]["mode"], json!("interleave"));
    assert_eq!(
        body["items"].as_array().map(Vec::len),
        body["counts"]["blended"].as_u64().map(|n| n as usize),
        "the answer's items are exactly the blend it reports: {body}"
    );

    let (status, body) = reader
        .send("DELETE", &format!("/api/v1/saved-view-blends/{id}"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, _) = reader
        .send(
            "GET",
            &format!("/api/v1/saved-view-blends/{id}/items"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_switch_closes_every_door_with_404_not_403() {
    let dir = scratch_dir("saved-filters-off");
    let tdb = TestDb::connect_with_dir("m69-21-off", &dir).await;
    let mut reader = Client::new(router_with(&dir, &tdb, false));
    reader.register("switch-reader").await;
    let view = create_view(&mut reader, "Any filter", "public").await;

    for (method, path) in [
        ("GET", "/api/v1/me/keybinds".to_owned()),
        ("GET", "/api/v1/me/followed-filters".to_owned()),
        ("GET", "/api/v1/saved-view-blends".to_owned()),
        ("PATCH", format!("/api/v1/saved-views/{view}/keybind")),
        ("POST", format!("/api/v1/saved-views/{view}/follow")),
    ] {
        let body = if method == "PATCH" {
            Some(json!({ "keybind": 1 }))
        } else {
            None
        };
        let (status, response) = reader.send(method, &path, body).await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "{method} {path} is closed when [discovery] saved_filters is off, and 404 rather than 403 says so \
             without confirming the surface: {response}"
        );
    }
}
