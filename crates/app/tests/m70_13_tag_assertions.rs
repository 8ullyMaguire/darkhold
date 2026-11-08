//! Tagging a work over HTTP — the doors `tag_assertions` was built for (M70-13 class B).
//!
//! Four rules, and each is only real if a request proves it:
//!
//! * a tag added by the work's author is `author`, by anyone else `curator` — derived from ownership, never from the request;
//! * a model's proposal is **withheld** from a reader and shown to an operator, and the answer says how many were withheld;
//! * a model cannot confirm: the confirmation door has no path that writes `inferred` + a confirmation;
//! * withdrawing a tag is the owner's or the operator's, and a stranger's refusal does not confirm the tag exists.

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

fn router(dir: &std::path::Path, tdb: &TestDb) -> axum::Router {
    let mut config = Config::development_defaults();
    config.storage.root = dir.to_path_buf();
    server::build_router(AppState::new(config, tdb.db().clone()))
}

async fn published(client: &mut Client, title: &str) -> String {
    let (status, body) = client
        .send("POST", "/api/v1/works", Some(json!({ "title": title })))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_str().expect("a work id").to_owned()
}

#[tokio::test]
async fn the_author_and_a_reader_are_told_apart_by_ownership() {
    let dir = scratch_dir("tag-assert");
    let tdb = TestDb::connect_with_dir("tag-assert", &dir).await;
    let mut author = Client::new(router(&dir, &tdb));
    author.register("tag-author").await;
    let work = published(&mut author, "Tagged Work").await;

    let mut reader = Client::new(router(&dir, &tdb));
    reader.register("tag-reader").await;

    let (status, body) = author
        .send(
            "POST",
            &format!("/api/v1/works/{work}/tags/angst/assert"),
            Some(json!({ "confidence": 0.9 })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["source"], json!("author"), "{body}");
    assert_eq!(body["confirmation"], json!("unconfirmed"));

    let (status, body) = reader
        .send(
            "POST",
            &format!("/api/v1/works/{work}/tags/slow-burn/assert"),
            None,
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "an empty body means confidence 1.0: {body}"
    );
    assert_eq!(body["source"], json!("curator"), "{body}");

    let (status, body) = reader
        .send("GET", &format!("/api/v1/works/{work}/tag-assertions"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let nodes: Vec<&str> = body["assertions"]
        .as_array()
        .expect("assertions")
        .iter()
        .filter_map(|row| row["node_id"].as_str())
        .collect();
    assert!(
        nodes.contains(&"angst") && nodes.contains(&"slow-burn"),
        "{nodes:?}"
    );
}

#[tokio::test]
async fn a_models_proposal_is_withheld_from_a_reader_and_named_as_withheld() {
    let dir = scratch_dir("tag-proposal");
    let tdb = TestDb::connect_with_dir("tag-proposal", &dir).await;
    let mut author = Client::new(router(&dir, &tdb));
    author.register("proposal-author").await;
    let work = published(&mut author, "Proposal Work").await;

    // What a model writes: `inferred` and unconfirmed. Written directly, because the doors deliberately cannot express it.
    let insert = tdb.db().sql(
        "INSERT INTO work_tags (work_id, node_id, source, confidence, role, prominence, confirmation, added_at)
         VALUES (?, 'fluff', 'inferred', 0.4, 'secondary', 0, 'unconfirmed', '2026-11-08T00:00:00Z')",
        "INSERT INTO work_tags (work_id, node_id, source, confidence, role, prominence, confirmation, added_at)
         VALUES ($1::uuid, 'fluff', 'inferred', 0.4, 'secondary', 0, 'unconfirmed', '2026-11-08T00:00:00Z')",
    );
    match tdb.db().backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(&insert)
                .bind(&work)
                .execute(tdb.db().sqlite_pool().expect("sqlite"))
                .await
                .expect("insert proposal");
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(&insert)
                .bind(&work)
                .execute(tdb.db().postgres_pool().expect("postgres"))
                .await
                .expect("insert proposal");
        }
    }

    let (status, body) = author
        .send("GET", &format!("/api/v1/works/{work}/tag-assertions"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["assertions"].as_array().map(Vec::len),
        Some(0),
        "a model's guess is not a tag: {body}"
    );
    assert_eq!(
        body["proposals_withheld"],
        json!(1),
        "and the reader is told something was withheld, not left to conclude the work has no tags: {body}"
    );
}

#[tokio::test]
async fn a_reader_confirms_and_a_wrangler_needs_the_operator() {
    let dir = scratch_dir("tag-confirm");
    let tdb = TestDb::connect_with_dir("tag-confirm", &dir).await;
    let mut reader = Client::new(router(&dir, &tdb));
    reader.register("confirm-reader").await;
    let work = published(&mut reader, "Confirmed Work").await;
    reader
        .send(
            "POST",
            &format!("/api/v1/works/{work}/tags/angst/assert"),
            None,
        )
        .await;

    let (status, body) = reader
        .send(
            "POST",
            &format!("/api/v1/works/{work}/tags/angst/confirm"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["confirmation"], json!("reader"));

    // Speaking for the taxonomy is the operator's, and the refusal is the instance's operator shape (`404`, ADR 0030) —
    // the tag is public, the operator's standing is not, so a wrangler refusal discloses nothing the tag did not.
    let (status, body) = reader
        .send(
            "POST",
            &format!("/api/v1/works/{work}/tags/angst/confirm"),
            Some(json!({ "level": "wrangler" })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    let (status, body) = reader
        .send(
            "POST",
            &format!("/api/v1/works/{work}/tags/angst/confirm"),
            Some(json!({ "level": "curator" })),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "not a level: {body}"
    );
}

#[tokio::test]
async fn only_the_owner_or_the_operator_withdraws_a_tag() {
    let dir = scratch_dir("tag-retract");
    let tdb = TestDb::connect_with_dir("tag-retract", &dir).await;
    let mut author = Client::new(router(&dir, &tdb));
    author.register("retract-author").await;
    let work = published(&mut author, "Retracted Work").await;
    author
        .send(
            "POST",
            &format!("/api/v1/works/{work}/tags/angst/assert"),
            None,
        )
        .await;

    let mut stranger = Client::new(router(&dir, &tdb));
    stranger.register("retract-stranger").await;
    let (status, body) = stranger
        .send("DELETE", &format!("/api/v1/works/{work}/tags/angst"), None)
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a stranger may not withdraw a tag, and learns nothing about whether one exists: {body}"
    );

    let (status, body) = author
        .send("DELETE", &format!("/api/v1/works/{work}/tags/angst"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["retracted"], json!(true));

    // A second withdrawal is a `404` — the tag does not exist, and saying so is the honest answer to a repeat.
    let (status, _) = author
        .send("DELETE", &format!("/api/v1/works/{work}/tags/angst"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
