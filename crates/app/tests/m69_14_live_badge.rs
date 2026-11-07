//! M69-14 — a live badge through the doors: gained by the affinity sweep, lost when the reading drifts, and both visible
//! to the reader with the number behind them.
//!
//! The whole feature is one loop — sweep → decide → event — so the test drives the loop rather than the pieces: run the
//! operator sweep with a reader whose taste matches the instance's, then change the inputs and run it again. A badge that
//! could only ever be *gained* would pass a unit test of `decide` and fail this file.

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
        "darkhold-m6914-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
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

    /// Swap the router under the same session, so a config change (here: who the operator is) does not cost the cookies.
    fn swap(&mut self, app: axum::Router) {
        self.app = app;
    }

    fn capture(&mut self, response: &axum::response::Response) {
        for value in response.headers().get_all(header::SET_COOKIE) {
            let Ok(text) = value.to_str() else { continue };
            let Some((pair, _)) = text.split_once(';') else { continue };
            let Some((name, value)) = pair.split_once('=') else { continue };
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
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.expect("body");
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    async fn register(&mut self, handle: &str) -> String {
        let (status, body) = self
            .send(
                "POST",
                "/api/v1/auth/register",
                Some(json!({
                    "email": format!("m69-14-{handle}@example.test"),
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

fn router(tdb: &TestDb, dir: &std::path::Path) -> axum::Router {
    let mut config = Config::development_defaults();
    config.storage.root = dir.to_path_buf();
    config.administration.operator_account_id = None;
    // The default config: `live_badges` is off here, and this router is the baseline the two tests measure against.
    server::build_router(AppState::new(config, tdb.db().clone()))
}

#[tokio::test]
async fn the_sweep_awards_the_badge_and_a_drift_takes_it_away_with_a_reason() {
    server::set_trust_proxy(false);
    let dir = scratch_dir("badge");
    let tdb = TestDb::connect_with_dir("m69-14-badge", &dir).await;

    let mut operator = Client::new(router(&tdb, &dir));
    let operator_id = operator.register("the-operator").await;

    // The instance's taste: two works that share a tag. One more it shrugs at.
    let valued = seed_work(&tdb, "The valued one", &["angst", "slowburn"]).await;
    let valued_too = seed_work(&tdb, "Valued too", &["angst"]).await;
    set_operator_affinity(&tdb, &valued, 9000).await;
    set_operator_affinity(&tdb, &valued_too, 9000).await;
    let shrugged = seed_work(&tdb, "Shrugged at", &["fluff"]).await;

    let mut reader = Client::new(router(&tdb, &dir));
    let reader_id = reader.register("attuned-reader").await;
    bookmark(&tdb, &reader_id, &valued).await;
    bookmark(&tdb, &reader_id, &valued_too).await;

    // The instance's operator is whoever the sweep measures against; point the config at that id and keep the session —
    // the route reads the operator from config, not from the session.
    operator.swap(router_with_operator(&tdb, &dir, &operator_id));

    let (status, body) = operator
        .send("POST", "/api/v1/operator/affinity/recompute", Some(json!({})))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["badges_gained"], json!(1), "a reader at the top of the scale is attuned: {body}");
    assert_eq!(body["badges_lost"], json!(0), "{body}");

    reader.swap(router_with_operator(&tdb, &dir, &operator_id));
    let (_, body) = reader.send("GET", "/api/v1/me/affinity", None).await;
    assert_eq!(body["scored"], json!(true), "{body}");

    let (status, body) = reader.send("GET", "/api/v1/me/badges", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let attuned = body["badges"]
        .as_array()
        .expect("badges")
        .iter()
        .find(|badge| badge["badge"] == "attuned")
        .expect("the live badge is listed");
    assert_eq!(attuned["held"], json!(true), "{body}");
    assert!(attuned["since"].is_string(), "{body}");
    assert_eq!(body["history"].as_array().expect("history").len(), 1, "{body}");

    // Now the reader's taste drifts: they drop what they had collected and collect only works the operator shrugged at,
    // so the favourite overlap goes to zero.
    unbookmark(&tdb, &reader_id, &valued).await;
    unbookmark(&tdb, &reader_id, &valued_too).await;
    bookmark(&tdb, &reader_id, &shrugged).await;
    let (status, body) = operator
        .send("POST", "/api/v1/operator/affinity/recompute", Some(json!({})))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["badges_lost"], json!(1), "the badge goes with the reading: {body}");

    let (_, body) = reader.send("GET", "/api/v1/me/badges", None).await;
    let attuned = body["badges"]
        .as_array()
        .expect("badges")
        .iter()
        .find(|badge| badge["badge"] == "attuned")
        .expect("still listed");
    assert_eq!(attuned["held"], json!(false), "{body}");
    assert!(attuned["since"].is_null(), "a badge not held has no 'since': {body}");

    let history = body["history"].as_array().expect("history");
    assert_eq!(history.len(), 2, "both transitions are recorded: {body}");
    assert_eq!(history[0]["kind"], "lost", "{body}");
    assert!(
        history[0]["reason"].as_str().expect("reason").contains("below"),
        "the reason names what failed: {body}"
    );
    assert!(history[0]["affinity_bp"].as_i64().expect("bp") < 7_500, "{body}");
}

#[tokio::test]
async fn off_by_default_the_door_answers_404_and_the_sweep_decides_nothing() {
    server::set_trust_proxy(false);
    let dir = scratch_dir("badge-off");
    let tdb = TestDb::connect_with_dir("m69-14-badge-off", &dir).await;

    // The default config: `[discovery] live_badges` is absent, so the feature is off.
    let mut client = Client::new(router(&tdb, &dir));
    let id = client.register("off-by-default").await;
    assert!(!id.is_empty());

    let (status, body) = client.send("GET", "/api/v1/me/badges", None).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a reader must not learn that this instance measures them and chose not to show it: {body}"
    );

    let operator_id = client.register("the-operator-off").await;
    client.swap(router_with_operator_off(&tdb, &dir, &operator_id));
    seed_work(&tdb, "Valued", &["angst"]).await;
    let (status, body) = client
        .send("POST", "/api/v1/operator/affinity/recompute", Some(json!({})))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["badges_gained"], json!(0), "{body}");
    assert_eq!(body["badges_lost"], json!(0), "{body}");

    // And nothing was decided behind the switch: no state row exists.
    let states = darkhold_db::live_badges::for_account(tdb.db(), &operator_id)
        .await
        .expect("states");
    for state in states {
        assert!(
            state.checked_at.is_empty(),
            "{} was decided while the feature was off",
            state.badge
        );
    }
}

fn router_with_operator_off(tdb: &TestDb, dir: &std::path::Path, operator: &str) -> axum::Router {
    let mut config = Config::development_defaults();
    config.storage.root = dir.to_path_buf();
    config.administration.operator_account_id = Some(operator.parse().expect("account id"));
    config.discovery.live_badges = false;
    server::build_router(AppState::new(config, tdb.db().clone()))
}

fn router_with_operator(tdb: &TestDb, dir: &std::path::Path, operator: &str) -> axum::Router {
    let mut config = Config::development_defaults();
    config.storage.root = dir.to_path_buf();
    config.administration.operator_account_id = Some(operator.parse().expect("account id"));
    config.discovery.live_badges = true;
    server::build_router(AppState::new(config, tdb.db().clone()))
}

async fn seed_work(tdb: &TestDb, title: &str, tags: &[&str]) -> String {
    let now = "2026-11-07T00:00:00Z";
    let pseud: String = sqlx::query_scalar("SELECT id FROM pseuds ORDER BY created_at LIMIT 1")
        .fetch_one(tdb.db().sqlite_pool().expect("sqlite"))
        .await
        .expect("a pseud exists");
    let owner: darkhold_domain::ids::PseudId = pseud.parse().expect("pseud id");
    let work = darkhold_db::content::create_work(tdb.db(), owner, title, None)
        .await
        .expect("create work");
    let id = work.id.to_string();
    for tag in tags {
        sqlx::query("INSERT OR IGNORE INTO work_tags (work_id, node_id, added_at) VALUES (?, ?, ?)")
            .bind(&id)
            .bind(format!("node-{tag}"))
            .bind(now)
            .execute(tdb.db().sqlite_pool().expect("sqlite"))
            .await
            .expect("tag");
    }
    id
}

async fn set_operator_affinity(tdb: &TestDb, work: &str, bp: i64) {
    sqlx::query(
        "INSERT INTO operator_affinities (work_id, affinity_bp, operator, rationale, set_at)
         VALUES (?, ?, 'operator', 'seed', '2026-11-07T00:00:00Z')",
    )
    .bind(work)
    .bind(bp)
    .execute(tdb.db().sqlite_pool().expect("sqlite"))
    .await
    .expect("operator affinity");
}

async fn unbookmark(tdb: &TestDb, account: &str, work: &str) {
    sqlx::query("DELETE FROM bookmarks WHERE account_id = ? AND subject_id = ?")
        .bind(account)
        .bind(work)
        .execute(tdb.db().sqlite_pool().expect("sqlite"))
        .await
        .expect("unbookmark");
}

async fn bookmark(tdb: &TestDb, account: &str, work: &str) {
    sqlx::query(
        "INSERT INTO bookmarks (id, account_id, subject_type, subject_id, note, is_public, created_at, updated_at)
         VALUES (?, ?, 'work', ?, '', 0, '2026-11-07T00:00:00Z', '2026-11-07T00:00:00Z')",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(account)
    .bind(work)
    .execute(tdb.db().sqlite_pool().expect("sqlite"))
    .await
    .expect("bookmark");
}
