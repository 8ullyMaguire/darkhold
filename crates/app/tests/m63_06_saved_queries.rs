//! M63-06: saved queries and the digest diff, through the real router.
//!
//! The store is covered in `crates/db/tests/saved_queries.rs` and the diff in
//! `darkhold-domain`. These are the route properties, and each one is a way the
//! feature could be present but useless:
//!
//! - a catalogue query is stored as the string the caller sent, so the page it
//!   answers later is the page it answered now — a re-encoded document would
//!   drift exactly where a window is anchored;
//! - the library shape still works, because a version 2 document is an
//!   extension of version 1, not a replacement (the existing library view tests
//!   are the other half of that guard);
//! - a view that sends both shapes, or neither, is refused by name rather than
//!   guessed at;
//! - `exclude_read` in a shared view is refused: the query would answer
//!   differently for every reader while claiming to be shareable;
//! - the first run is all-new, the second with the same page is empty, and a row
//!   that entered the answer since is the only thing a third run reports;
//! - a run against someone else's view is 404, not an empty diff.

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::Database;
use serde_json::Value;
use test_support::TestDb;
use tower::ServiceExt;

const GOOD_PASSWORD: &str = "a-long-enough-passphrase";

struct Harness {
    _tdb: TestDb,
    _dir: std::path::PathBuf,
    config: Config,
    db: Database,
}

impl Harness {
    async fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "darkhold-m63-06-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        let mut config = Config::development_defaults();
        config.storage.root = dir.clone();
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
        Client {
            app: server::build_router(AppState::new(self.config.clone(), self.db.clone())),
            cookies: Vec::new(),
        }
    }
}

struct Client {
    app: axum::Router,
    cookies: Vec<(String, String)>,
}

impl Client {
    async fn request(
        &mut self,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder().method(method).uri(uri);
        let cookies = self
            .cookies
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; ");
        if !cookies.is_empty() {
            builder = builder.header(header::COOKIE, cookies);
        }
        if !matches!(method, "GET" | "HEAD" | "OPTIONS") {
            if let Some((_, token)) = self.cookies.iter().find(|(k, _)| k == "darkhold_csrf") {
                builder = builder.header("x-csrf-token", token.clone());
            }
        }
        let request = match body {
            Some(ref value) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(value).expect("serialise")))
                .expect("request"),
            None => builder.body(Body::empty()).expect("request"),
        };
        let response = self.app.clone().oneshot(request).await.expect("response");
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
        let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
            .await
            .expect("body");
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes)
                .unwrap_or(Value::String(String::from_utf8_lossy(&bytes).into_owned()))
        };
        (status, value)
    }

    async fn get(&mut self, uri: &str) -> (StatusCode, Value) {
        self.request("GET", uri, None).await
    }

    async fn post(&mut self, uri: &str, body: Value) -> (StatusCode, Value) {
        self.request("POST", uri, Some(body)).await
    }
}

async fn reader(harness: &Harness, handle: &str) -> Client {
    let mut client = harness.client();
    let (status, body) = client
        .post(
            "/api/v1/auth/register",
            serde_json::json!({
                "email": format!("{handle}@example.com"),
                "password": GOOD_PASSWORD,
                "handle": handle,
                "display_name": handle,
                "age_band": "adult",
            }),
        )
        .await;
    assert!(status.is_success(), "register {handle}: {status} {body}");
    client
}

/// A catalogue query as `dh rec` would spell it.
const PARAMS: &str = "fandom=naruto&updated=7d&sort=kudos&limit=2";

async fn save_catalogue(client: &mut Client, name: &str) -> Value {
    let (status, body) = client
        .post(
            "/api/v1/saved-views",
            serde_json::json!({
                "name": name,
                "kind": "catalogue",
                "params": PARAMS,
                // What `dh rec --save` sends: a catalogue query has no private
                // inputs, so the view is shareable.
                "scope": "public",
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "save {name}: {body}");
    body
}

async fn run(client: &mut Client, id: &str, ids: &[&str]) -> (StatusCode, Value) {
    client
        .post(
            &format!("/api/v1/saved-views/{id}/runs"),
            serde_json::json!({ "ids": ids }),
        )
        .await
}

#[tokio::test]
async fn a_catalogue_view_stores_the_string_it_was_given() {
    let harness = Harness::new("store").await;
    let mut client = reader(&harness, "m6306store").await;

    let body = save_catalogue(&mut client, "weekly naruto").await;
    assert_eq!(body["kind"], "catalogue");
    assert_eq!(
        body["params"], PARAMS,
        "the params must come back byte-identical: {body}"
    );
    assert!(body["query"].is_null(), "{}", body["query"]);
    assert_eq!(body["needs_repair"], false);
    assert_eq!(body["scope"], "public");
    assert_eq!(body["version"], 1);
}

#[tokio::test]
async fn the_stored_string_is_the_one_a_later_read_replays() {
    let harness = Harness::new("replay").await;
    let mut client = reader(&harness, "m6306replay").await;
    let saved = save_catalogue(&mut client, "weekly naruto").await;

    let (status, listed) = client.get("/api/v1/saved-views").await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    let item = listed["items"]
        .as_array()
        .expect("items")
        .iter()
        .find(|item| item["id"] == saved["id"])
        .expect("the view is listed");
    assert_eq!(
        item["params"], PARAMS,
        "a listing is what the CLI replays from: {item}"
    );
}

#[tokio::test]
async fn a_library_view_still_stores_a_query_and_reads_back() {
    let harness = Harness::new("library").await;
    let mut client = reader(&harness, "m6306library").await;

    let (status, body) = client
        .post(
            "/api/v1/saved-views",
            serde_json::json!({
                "name": "unread naruto",
                "query": { "tags": ["naruto"] },
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["kind"], "library", "{body}");
    assert_eq!(body["query"]["tags"][0], "naruto", "{body}");
    assert!(body["params"].is_null(), "{body}");
    assert_eq!(
        body["query_version"], 2,
        "a library view written by this build is a version 2 document: {body}"
    );

    let id = body["id"].as_str().expect("id").to_owned();
    let (status, read) = client.get(&format!("/api/v1/saved-views/{id}")).await;
    assert_eq!(status, StatusCode::OK, "{read}");
    assert_eq!(
        read["query"]["tags"][0], "naruto",
        "the document decodes back: {read}"
    );
    assert_eq!(read["needs_repair"], false);
}

#[tokio::test]
async fn a_shape_and_a_kind_that_disagree_are_refused() {
    let harness = Harness::new("disagree").await;
    let mut client = reader(&harness, "m6306disagree").await;

    // Both shapes: the server cannot know which one the caller meant, and picking
    // one would silently drop the other. The library `query` here carries only a
    // key `LibraryQuery` does not know (`q`), which is the point: presence is what
    // is refused, not content, or a typo would be silently dropped instead.
    let (status, body) = client
        .post(
            "/api/v1/saved-views",
            serde_json::json!({
                "name": "both",
                "kind": "catalogue",
                "params": PARAMS,
                "query": { "q": "naruto" },
            }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("params"),
        "the refusal must name the shape: {body}"
    );

    let (status, body) = client
        .post(
            "/api/v1/saved-views",
            serde_json::json!({ "name": "neither", "kind": "library", "params": PARAMS }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("params"),
        "{body}"
    );

    // A catalogue kind with no params is a view that cannot be run.
    let (status, body) = client
        .post(
            "/api/v1/saved-views",
            serde_json::json!({ "name": "empty", "kind": "catalogue" }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("params"),
        "{body}"
    );
}

#[tokio::test]
async fn an_unknown_kind_is_named_rather_than_defaulted() {
    let harness = Harness::new("kind").await;
    let mut client = reader(&harness, "m6306kind").await;

    let (status, body) = client
        .post(
            "/api/v1/saved-views",
            serde_json::json!({ "name": "x", "kind": "widget", "params": PARAMS }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let message = body["error"]["message"].as_str().unwrap_or_default();
    assert!(message.contains("widget"), "{message}");
    assert!(message.contains("catalogue"), "{message}");
}

#[tokio::test]
async fn a_query_that_reads_history_cannot_be_shared() {
    let harness = Harness::new("history").await;
    let mut client = reader(&harness, "m6306history").await;

    let (status, body) = client
        .post(
            "/api/v1/saved-views",
            serde_json::json!({
                "name": "unread",
                "kind": "catalogue",
                "params": "fandom=naruto&exclude_read=1&limit=2",
                "scope": "public",
            }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("exclude_read"),
        "{body}"
    );

    // The same query scoped to the library is fine: it depends on one account
    // because it says so.
    let (status, body) = client
        .post(
            "/api/v1/saved-views",
            serde_json::json!({
                "name": "unread",
                "kind": "catalogue",
                "params": "fandom=naruto&exclude_read=1&limit=2",
                "scope": "library",
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["scope"], "library");
}

#[tokio::test]
async fn the_first_run_is_all_new_and_a_repeat_is_none() {
    let harness = Harness::new("runs").await;
    let mut client = reader(&harness, "m6306runs").await;
    let saved = save_catalogue(&mut client, "weekly naruto").await;
    let id = saved["id"].as_str().expect("id");

    let (status, first) = run(&mut client, id, &["a", "b"]).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["new_ids"], serde_json::json!(["a", "b"]));
    assert_eq!(
        first["previous"], 0,
        "nothing was recorded before the first run: {first}"
    );
    assert!(
        first["ran_at"].as_str().is_some_and(|s| !s.is_empty()),
        "{first}"
    );

    let (status, second) = run(&mut client, id, &["a", "b"]).await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["new_ids"], serde_json::json!([]));
    assert_eq!(second["previous"], 2);
}

#[tokio::test]
async fn a_row_that_entered_the_answer_since_is_the_only_thing_reported() {
    let harness = Harness::new("entered").await;
    let mut client = reader(&harness, "m6306entered").await;
    let saved = save_catalogue(&mut client, "weekly naruto").await;
    let id = saved["id"].as_str().expect("id");

    run(&mut client, id, &["a", "b"]).await;
    // `b` survives, `a` left the window, `c` entered — a diff by creation date
    // would report none of these, which is why the ids are what is compared.
    let (status, third) = run(&mut client, id, &["c", "b"]).await;
    assert_eq!(status, StatusCode::OK, "{third}");
    assert_eq!(third["new_ids"], serde_json::json!(["c"]), "{third}");
    assert_eq!(third["previous"], 2);
}

#[tokio::test]
async fn a_run_against_someone_elses_view_is_not_an_empty_diff() {
    let harness = Harness::new("theirs").await;
    let mut owner = reader(&harness, "m6306owner").await;
    let saved = save_catalogue(&mut owner, "weekly naruto").await;
    let id = saved["id"].as_str().expect("id").to_owned();

    let mut stranger = reader(&harness, "m6306stranger").await;
    let (status, body) = run(&mut stranger, &id, &["a"]).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a view id is not a capability: {body}"
    );

    // And the owner's memory was not touched by the attempt.
    let (status, owner_run) = run(&mut owner, &id, &["a"]).await;
    assert_eq!(status, StatusCode::OK, "{owner_run}");
    assert_eq!(
        owner_run["previous"], 0,
        "the stranger's run must not have recorded anything: {owner_run}"
    );
}
