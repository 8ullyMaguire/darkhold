//! Authoring a work's cast over HTTP — the write half of `work_characters` (M70-13).
//!
//! The store could always do this; nothing could reach it. Five rules, and each is only real if a request proves it:
//!
//! * the work's **owner** authors the cast, and a stranger's write is a `404` that does not confirm the work exists;
//! * a dismissed claim (`DELETE`) answers `404` the second time, because "withdrawn" and "there was nothing" are different
//!   answers;
//! * a pairing carries its ends and its dynamics in one write, because half a pairing is not a claim anyone made;
//! * the pairing's ends are keyed by the pairing **node** and its dynamics by the claim's **row id** — two different strings
//!   for one relationship, which is the mistake the read door's first test caught;
//! * the read door folds roles and attributes in, so a cast list is one request rather than one per character.

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
        "darkhold-m7013cast-{tag}-{}-{:?}",
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

    async fn register(&mut self, handle: &str) {
        let (status, body) = self
            .send(
                "POST",
                "/api/v1/auth/register",
                Some(json!({
                    "email": format!("m7013cast-{handle}@example.test"),
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

/// The nodes the store's foreign keys require. All four live in `canonical_entities`: migration 0124 renamed
/// `taxonomy_nodes` to it, so 0103's `attribute_node_id` reference now points there and a fixture that seeds the old table
/// name seeds nothing.
async fn seed_nodes(tdb: &TestDb) {
    let db = tdb.db();
    for (id, kind) in [
        ("ada", "character"),
        ("grace", "character"),
        ("ada-and-grace", "relationship"),
        ("protagonist", "character_role"),
        ("analytical", "character_attribute"),
        ("slow-burn", "relationship_dynamic"),
    ] {
        let sql = db.sql(
            "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at, review_status, signal_count) \
             VALUES (?1, ?2, ?1, ?1, '2026-01-01T00:00:00Z', 'curated', 0)",
            "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at, review_status, signal_count) \
             VALUES ($1, $2, $1, $1, '2026-01-01T00:00:00Z', 'curated', 0)",
        );
        match db.backend() {
            darkhold_db::Backend::Sqlite => {
                sqlx::query(&sql)
                    .bind(id)
                    .bind(kind)
                    .execute(db.sqlite_pool().expect("sqlite handle"))
                    .await
                    .expect("seed canonical entity");
            }
            darkhold_db::Backend::Postgres => {
                sqlx::query(&sql)
                    .bind(id)
                    .bind(kind)
                    .execute(db.postgres_pool().expect("postgres handle"))
                    .await
                    .expect("seed canonical entity");
            }
        }
    }
}

#[tokio::test]
async fn the_owner_authors_the_cast_and_a_stranger_cannot() {
    let dir = scratch_dir("cast");
    let tdb = TestDb::connect_with_dir("m7013cast", &dir).await;
    seed_nodes(&tdb).await;

    let mut author = Client::new(router(&dir, &tdb));
    author.register("cast-author").await;
    let work = published(&mut author, "A Work With A Cast").await;

    // The claim itself.
    let (status, body) = author
        .send(
            "PUT",
            &format!("/api/v1/works/{work}/characters/ada"),
            Some(json!({ "prominence": "protagonist", "is_pov": true })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["created"],
        json!(true),
        "first claim is a create: {body}"
    );

    // Re-claiming updates rather than duplicating: `created` is false and the read shows one row.
    let (status, body) = author
        .send(
            "PUT",
            &format!("/api/v1/works/{work}/characters/ada"),
            Some(json!({ "prominence": "supporting", "is_pov": false })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["created"], json!(false), "{body}");
    assert_eq!(
        body["character"]["prominence"],
        json!("supporting"),
        "{body}"
    );

    // Roles and attributes, folded into the cast read.
    for path in [
        format!("/api/v1/works/{work}/characters/ada/roles/protagonist"),
        format!("/api/v1/works/{work}/characters/ada/attributes/analytical"),
    ] {
        let (status, body) = author.send("PUT", &path, None).await;
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
    }

    // The pairing: ends and dynamics in one write.
    let (status, body) = author
        .send(
            "PUT",
            &format!("/api/v1/works/{work}/relationships/ada-and-grace"),
            Some(json!({
                "rel_type": "romantic",
                "prominence": "secondary",
                "label": "the slow one",
                "participants": ["ada", "grace"],
                "dynamics": ["slow-burn"],
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = author
        .send("GET", &format!("/api/v1/works/{work}/characters"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["characters"].as_array().expect("cast").len(),
        1,
        "{body}"
    );
    assert_eq!(
        body["characters"][0]["prominence"],
        json!("supporting"),
        "{body}"
    );
    assert_eq!(body["characters"][0]["is_pov"], json!(false), "{body}");
    let roles = body["characters"][0]["roles"].as_array().expect("roles");
    assert_eq!(roles.len(), 1, "roles are folded in: {body}");
    let attributes = body["characters"][0]["attributes"]
        .as_array()
        .expect("attributes");
    assert_eq!(attributes.len(), 1, "attributes are folded in: {body}");

    let relationships = body["relationships"].as_array().expect("relationships");
    assert_eq!(relationships.len(), 1, "{body}");
    assert_eq!(relationships[0]["rel_type"], json!("romantic"), "{body}");
    assert_eq!(relationships[0]["label"], json!("the slow one"), "{body}");
    let ends: Vec<&str> = relationships[0]["participants"]
        .as_array()
        .expect("participants")
        .iter()
        .map(|v| v.as_str().expect("id"))
        .collect();
    assert_eq!(ends.len(), 2, "both ends survive the write: {body}");
    assert!(ends.contains(&"ada") && ends.contains(&"grace"), "{body}");
    let dynamics = relationships[0]["dynamics"].as_array().expect("dynamics");
    assert_eq!(dynamics.len(), 1, "dynamics survive the write: {body}");

    // The fine-grained reads agree with the folded ones.
    let (status, body) = author
        .send(
            "GET",
            &format!("/api/v1/works/{work}/characters/ada/roles"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["roles"].as_array().expect("roles").len(), 1, "{body}");

    let (status, body) = author
        .send(
            "GET",
            &format!("/api/v1/works/{work}/relationships/ada-and-grace/dynamics"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["dynamics"].as_array().expect("dynamics").len(),
        1,
        "{body}"
    );

    // A stranger: every write is a 404, and nothing they sent is stored.
    let mut stranger = Client::new(router(&dir, &tdb));
    stranger.register("cast-stranger").await;
    let (status, body) = stranger
        .send(
            "PUT",
            &format!("/api/v1/works/{work}/characters/grace"),
            Some(json!({ "prominence": "protagonist", "is_pov": true })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, body) = stranger
        .send(
            "DELETE",
            &format!("/api/v1/works/{work}/characters/ada"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    let (_, body) = author
        .send("GET", &format!("/api/v1/works/{work}/characters"), None)
        .await;
    assert_eq!(
        body["characters"].as_array().expect("cast").len(),
        1,
        "a stranger wrote nothing: {body}"
    );
    assert_eq!(
        body["characters"][0]["character_node_id"],
        json!("ada"),
        "and deleted nothing: {body}"
    );

    // Withdrawal answers once, then not-found — and takes the pairing with it, because the pairing's ends are that
    // character.
    let (status, body) = author
        .send(
            "DELETE",
            &format!("/api/v1/works/{work}/characters/ada"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = author
        .send(
            "DELETE",
            &format!("/api/v1/works/{work}/characters/ada"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

#[tokio::test]
async fn a_pairing_with_one_end_is_refused() {
    let dir = scratch_dir("one-end");
    let tdb = TestDb::connect_with_dir("m7013oneend", &dir).await;
    seed_nodes(&tdb).await;

    let mut author = Client::new(router(&dir, &tdb));
    author.register("one-end-author").await;
    let work = published(&mut author, "Half A Pairing").await;

    let (status, body) = author
        .send(
            "PUT",
            &format!("/api/v1/works/{work}/relationships/ada-and-grace"),
            Some(json!({
                "rel_type": "romantic",
                "prominence": "secondary",
                "participants": ["ada"],
            })),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a pairing has two ends: {body}"
    );

    // And a vocabulary the store would refuse is refused at the door, with the field named.
    let (status, body) = author
        .send(
            "PUT",
            &format!("/api/v1/works/{work}/relationships/ada-and-grace"),
            Some(json!({
                "rel_type": "sworn-enemies",
                "prominence": "secondary",
                "participants": ["ada", "grace"],
            })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    let (_, body) = author
        .send("GET", &format!("/api/v1/works/{work}/characters"), None)
        .await;
    assert_eq!(
        body["relationships"]
            .as_array()
            .expect("relationships")
            .len(),
        0,
        "a refused write stores nothing: {body}"
    );
}
