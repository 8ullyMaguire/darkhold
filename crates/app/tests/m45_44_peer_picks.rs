//! M45-44 — sister instances, both halves: what this instance publishes, and what it subscribes to.
//!
//! The publishing half is a document; the subscribing half is a sweep over a peer's document. Both are proven here, and the
//! fetch is proven against a **real HTTP server** on a loopback port rather than a mocked client — a mocked client proves the
//! parser was called, not that a collection a peer actually serves is read.

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::Database;
use serde_json::{json, Value};
use test_support::TestDb;
use tower::ServiceExt;

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-picks-{tag}-{}-{:?}",
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

    async fn send(&mut self, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut builder = Request::builder()
            .method(Method::from_bytes(method.as_bytes()).expect("method"))
            .uri(uri);
        if !self.cookies.is_empty() {
            let cookie = self
                .cookies
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect::<Vec<String>>()
                .join("; ");
            builder = builder.header(header::COOKIE, cookie);
        }
        if let Some((_, token)) = self.cookies.iter().find(|(k, _)| k == "darkhold_csrf") {
            builder = builder.header("x-csrf-token", token.clone());
        }
        let body = match body {
            Some(value) => {
                builder = builder.header(header::CONTENT_TYPE, "application/json");
                Body::from(value.to_string())
            }
            None => Body::empty(),
        };
        let response = self
            .app
            .clone()
            .oneshot(builder.body(body).expect("request"))
            .await
            .expect("response");
        let status = response.status();
        for value in response.headers().get_all(header::SET_COOKIE) {
            let Ok(raw) = value.to_str() else { continue };
            let pair = raw.split(';').next().unwrap_or(raw);
            let Some((name, val)) = pair.split_once('=') else {
                continue;
            };
            self.cookies.retain(|(key, _)| key != name);
            if !val.is_empty() {
                self.cookies.push((name.to_owned(), val.to_owned()));
            }
        }
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
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
                    "email": format!("picks-{handle}@example.test"),
                    "password": "test-pass-1234",
                    "handle": handle,
                    "display_name": handle,
                    "age_band": "adult",
                })),
            )
            .await;
        assert!(status.is_success(), "register {handle}: {body}");
        let (_, me) = self.send("GET", "/api/v1/auth/me", None).await;
        me["account"]["id"]
            .as_str()
            .or_else(|| me["id"].as_str())
            .expect("account id")
            .to_owned()
    }
}

fn base_config(dir: &std::path::Path) -> Config {
    let mut config = Config::development_defaults();
    config.storage.root = dir.to_path_buf();
    config
}

/// Serve one fixed document on a loopback port and return its URL plus the handle that stops it.
async fn serve(document: Value) -> (String, tokio::task::JoinHandle<()>) {
    let body = Arc::new(document.to_string());
    let app = Router::new().route(
        "/picks",
        axum::routing::get(move || {
            let body = body.clone();
            async move {
                (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "application/activity+json")],
                    (*body).clone(),
                )
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a loopback port");
    let addr = listener.local_addr().expect("the bound address");
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}/picks"), handle)
}

#[tokio::test]
async fn the_instance_publishes_a_collection_that_does_not_lie_about_its_length() {
    let dir = scratch_dir("publish");
    let tdb = TestDb::connect_with_dir("pickspub", &dir).await;
    let app = server::build_router(AppState::new(base_config(&dir), tdb.db().clone()));
    let mut client = Client::new(app);
    let (status, body) = client.send("GET", "/federation/picks", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["type"], json!("OrderedCollection"), "{body}");
    assert!(
        body["id"]
            .as_str()
            .unwrap_or_default()
            .ends_with("/federation/picks"),
        "{body}"
    );
    let items = body["orderedItems"].as_array().expect("orderedItems");
    assert_eq!(
        body["totalItems"].as_u64(),
        Some(items.len() as u64),
        "the count is the count carried: {body}"
    );
}

#[tokio::test]
async fn subscribing_is_the_operators_and_the_fetch_replaces_rather_than_accumulates() {
    let dir = scratch_dir("subscribe");
    let tdb = TestDb::connect_with_dir("pickssub", &dir).await;
    let db: Database = tdb.db().clone();
    let config = base_config(&dir);

    let mut first = Client::new(server::build_router(AppState::new(
        config.clone(),
        db.clone(),
    )));
    let operator_account = first.register("picksoperator").await;

    let mut operator_config = config.clone();
    operator_config.administration.operator_account_id =
        Some(operator_account.parse().expect("account id"));
    let mut operator = Client::new(server::build_router(AppState::new(
        operator_config,
        db.clone(),
    )));
    operator.cookies = first.cookies.clone();

    // A plain reader cannot see or change the subscriptions, and the refusal does not confirm the door exists.
    let mut reader = Client::new(server::build_router(AppState::new(
        config.clone(),
        db.clone(),
    )));
    reader.register("picksreader").await;
    let (status, body) = reader.send("GET", "/operator/federation/picks", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // The door refuses a URL that is not https, and one filed under a different host than it points at.
    let (status, body) = operator
        .send(
            "PUT",
            "/operator/federation/picks",
            Some(json!({ "peer_host": "peer.example", "collection_url": "http://peer.example/picks" })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let (status, body) = operator
        .send(
            "PUT",
            "/operator/federation/picks",
            Some(json!({ "peer_host": "peer.example", "collection_url": "https://elsewhere.example/picks" })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    // A subscription whose document is served by a real server. Inserted through the store because the door requires https and
    // a loopback test server cannot speak it — the door's own rule is proven above, and this test is about the fetch.
    let first_collection = json!({
        "type": "OrderedCollection",
        "orderedItems": [
            { "id": "https://peer.example/w/1", "name": "First Pick", "attributedTo": "peer-author" },
            { "id": "https://peer.example/w/2", "name": "Second Pick" }
        ]
    });
    let (url, handle) = serve(first_collection).await;
    let host = url
        .trim_start_matches("http://")
        .split('/')
        .next()
        .expect("host")
        .to_owned();
    let sub_id = darkhold_db::peer_picks::subscribe(&db, &host, &url)
        .await
        .expect("a subscription");

    let state = AppState::new(config.clone(), db.clone());
    let stored = darkhold_app::peer_picks::refresh_one(&state, &sub_id, &url)
        .await
        .expect("the fetch succeeds");
    assert_eq!(stored, 2, "two picks stored");

    let picks = darkhold_db::peer_picks::recent_picks(&db, 10)
        .await
        .expect("picks");
    assert_eq!(picks.len(), 2, "{picks:?}");
    assert_eq!(picks[0].peer_host, host, "{picks:?}");
    assert!(
        picks
            .iter()
            .any(|pick| pick.author.as_deref() == Some("peer-author")),
        "attribution survives the round trip: {picks:?}"
    );

    // The peer's next answer carries one pick. The old one must go: this table is the peer's current list, not an archive.
    let second_collection = json!({
        "orderedItems": [
            { "id": "https://peer.example/w/2", "name": "Second Pick" }
        ]
    });
    let (url2, handle2) = serve(second_collection).await;
    assert_eq!(
        darkhold_app::peer_picks::refresh_one(&state, &sub_id, &url2)
            .await
            .expect("the second fetch succeeds"),
        1
    );
    let picks = darkhold_db::peer_picks::recent_picks(&db, 10)
        .await
        .expect("picks");
    assert_eq!(picks.len(), 1, "the dropped pick is gone: {picks:?}");
    assert_eq!(picks[0].work_url, "https://peer.example/w/2");

    // A peer that goes quiet is visible in the operator's list rather than only in a log.
    let subscriptions = darkhold_db::peer_picks::subscriptions(&db)
        .await
        .expect("subscriptions");
    assert_eq!(subscriptions.len(), 1);
    assert_eq!(subscriptions[0].last_status.as_deref(), Some("1 picks"));

    // A failed fetch records why and leaves the picks alone.
    let before = picks.len();
    let error = darkhold_app::peer_picks::refresh_one(&state, &sub_id, "http://127.0.0.1:1/picks")
        .await
        .expect_err("an unreachable peer is an error");
    assert!(!error.is_empty(), "the reason is named");
    let subscriptions = darkhold_db::peer_picks::subscriptions(&db)
        .await
        .expect("subscriptions");
    assert!(
        subscriptions[0]
            .last_status
            .as_deref()
            .unwrap_or_default()
            .starts_with("the peer answered")
            || subscriptions[0]
                .last_status
                .as_deref()
                .unwrap_or_default()
                .contains("could not"),
        "the status says what happened: {:?}",
        subscriptions[0].last_status
    );
    assert!(
        subscriptions[0].last_error_at.is_some(),
        "a failure is timestamped: {:?}",
        subscriptions[0]
    );
    let after = darkhold_db::peer_picks::recent_picks(&db, 10)
        .await
        .expect("picks");
    assert_eq!(after.len(), before, "a failed fetch changes no picks");

    // Disabling the subscription hides its picks without deleting them: turning a peer off is reversible.
    let sql = db.sql(
        "UPDATE federation_pick_subscriptions SET enabled = 0 WHERE id = ?1",
        "UPDATE federation_pick_subscriptions SET enabled = FALSE WHERE id = $1::uuid",
    );
    match db.backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(&sub_id)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("disable");
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(&sql)
                .bind(&sub_id)
                .execute(db.postgres_pool().expect("postgres"))
                .await
                .expect("disable");
        }
    }
    let hidden = darkhold_db::peer_picks::recent_picks(&db, 10)
        .await
        .expect("picks");
    assert!(
        hidden.is_empty(),
        "a disabled peer's picks stop being shown: {hidden:?}"
    );

    // The public reader surface answers an empty list rather than a 404: "no sister instances yet" is a state, not an absence.
    let (status, body) = reader.send("GET", "/discovery/peer-picks", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["picks"], json!([]), "{body}");

    // Removing the subscription is a 404 for a host that is not subscribed.
    let (status, body) = operator
        .send("DELETE", "/operator/federation/picks/nosuch.example", None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    handle.abort();
    handle2.abort();
}

#[tokio::test]
async fn the_directory_finds_instances_by_topic_and_omits_the_ones_that_do_not_name_it() {
    let dir = scratch_dir("directory");
    let tdb = TestDb::connect_with_dir("picksdir", &dir).await;
    let db: Database = tdb.db().clone();
    let app = server::build_router(AppState::new(base_config(&dir), db.clone()));
    let mut client = Client::new(app);

    let now = darkhold_db::identity::now_rfc3339();
    // Two public themes naming the topic at different weights, one public theme that does not, and one private theme that does.
    darkhold_db::instance_theme::upsert_theme(
        &db,
        "strong.example",
        &json!({ "space opera": 0.8, "fluff": 0.2 }),
        true,
        &now,
    )
    .await
    .expect("theme");
    darkhold_db::instance_theme::upsert_theme(
        &db,
        "weak.example",
        &json!({ "space opera": 0.3 }),
        true,
        &now,
    )
    .await
    .expect("theme");
    darkhold_db::instance_theme::upsert_theme(
        &db,
        "other.example",
        &json!({ "cozy mystery": 0.9 }),
        true,
        &now,
    )
    .await
    .expect("theme");
    darkhold_db::instance_theme::upsert_theme(
        &db,
        "private.example",
        &json!({ "space opera": 0.99 }),
        false,
        &now,
    )
    .await
    .expect("theme");

    let (status, body) = client
        .send("GET", "/federation/directory?topic=Space%20Opera", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let instances = body["instances"].as_array().expect("instances");
    let ids: Vec<&str> = instances
        .iter()
        .filter_map(|row| row["instance_id"].as_str())
        .collect();
    assert_eq!(
        ids,
        vec!["strong.example", "weak.example"],
        "strongest first, unlisted and private omitted: {body}"
    );
    assert_eq!(instances[0]["weight"].as_f64(), Some(0.8));
    assert_eq!(
        instances[0]["topics_named"].as_u64(),
        Some(2),
        "the row says how selective the weight is: {body}"
    );
    assert!(
        !ids.contains(&"private.example"),
        "a private theme is not in a public directory: {body}"
    );

    // A topic nobody names is an empty list, not every instance: a directory that answers "all" to "anything" is not a search.
    let (status, body) = client
        .send(
            "GET",
            "/federation/directory?topic=nobody%20names%20this",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["total"].as_u64(), Some(0), "{body}");

    // And a search with no topic is refused rather than silently answering the whole directory.
    let (status, body) = client.send("GET", "/federation/directory", None).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
}
