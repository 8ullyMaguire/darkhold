//! M63-14: a saved view as a feed, through the real router.
//!
//! The document itself is covered in `crates/domain/tests/feed.rs`. These are the properties only the route
//! can have, and each is a way a feed could be present and useless:
//!
//! - a second poll that changed nothing costs no bytes — `If-None-Match` answers `304`, and the tag is stable
//!   because nothing in the document is a clock reading;
//! - a feed reader cannot send an `Authorization` header, so `?token=` reaches the same scope check a header
//!   does — and a wrong token is still a wrong token, not a 500;
//! - the view is the account's: another reader asking for the same id gets `404`, and an anonymous caller is
//!   asked to authenticate;
//! - an unknown `format` is refused by name instead of guessed at.

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
            "darkhold-m63-14-{tag}-{}-{:?}",
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

/// What a feed response is checked for: the status, every header, and the raw body.
struct Reply {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: String,
}

impl Reply {
    fn header(&self, name: header::HeaderName) -> &str {
        self.headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
    }
}

impl Client {
    async fn raw(
        &mut self,
        method: &str,
        uri: &str,
        body: Option<Value>,
        extra: &[(&str, &str)],
    ) -> Reply {
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
        for (name, value) in extra {
            builder = builder.header(*name, *value);
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
        let headers = response.headers().clone();
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
        Reply {
            status,
            headers,
            body: String::from_utf8_lossy(&bytes).into_owned(),
        }
    }

    async fn request(
        &mut self,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let reply = self.raw(method, uri, body, &[]).await;
        let value = if reply.body.is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&reply.body).unwrap_or(Value::String(reply.body.clone()))
        };
        (reply.status, value)
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

// No `fandom`: this harness seeds no catalogue rows, and a view naming an unknown fandom is refused
// (correctly) by the catalogue route. The feed renders whatever the query answers, including nothing.
const PARAMS: &str = "sort=kudos&limit=2";

async fn save_catalogue(client: &mut Client, name: &str) -> String {
    let (status, body) = client
        .post(
            "/api/v1/saved-views",
            serde_json::json!({
                "name": name,
                "kind": "catalogue",
                "params": PARAMS,
                "scope": "public",
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "save {name}: {body}");
    body["id"].as_str().expect("id").to_owned()
}

#[tokio::test]
async fn a_saved_view_renders_three_ways_and_a_second_poll_costs_nothing() {
    let harness = Harness::new("render").await;
    let mut client = reader(&harness, "feedreader").await;
    let id = save_catalogue(&mut client, "grimdark & long").await;

    let atom = client
        .raw("GET", &format!("/api/v1/saved-views/{id}/feed"), None, &[])
        .await;
    assert_eq!(atom.status, StatusCode::OK, "{}", atom.body);
    assert!(
        atom.header(header::CONTENT_TYPE)
            .starts_with("application/atom+xml"),
        "content type: {}",
        atom.header(header::CONTENT_TYPE)
    );
    // The view's own name, escaped and carried: the reader shows it as the feed's title.
    assert!(atom.body.contains("grimdark &amp; long"), "{}", atom.body);
    let tag = atom.header(header::ETAG).to_owned();
    assert!(tag.starts_with('"'), "a strong tag: {tag}");
    assert!(
        atom.header(header::CACHE_CONTROL).contains("private"),
        "a library feed is the reader's: {}",
        atom.header(header::CACHE_CONTROL)
    );

    let again = client
        .raw("GET", &format!("/api/v1/saved-views/{id}/feed"), None, &[])
        .await;
    assert_eq!(
        again.header(header::ETAG),
        tag,
        "same query, same bytes, same tag"
    );

    let conditional = client
        .raw(
            "GET",
            &format!("/api/v1/saved-views/{id}/feed"),
            None,
            &[("if-none-match", &tag)],
        )
        .await;
    assert_eq!(
        conditional.status,
        StatusCode::NOT_MODIFIED,
        "{}",
        conditional.body
    );
    assert!(conditional.body.is_empty(), "a 304 carries no document");
    assert_eq!(
        conditional.header(header::ETAG),
        tag,
        "the 304 repeats the tag"
    );

    let json = client
        .raw(
            "GET",
            &format!("/api/v1/saved-views/{id}/feed?format=json"),
            None,
            &[],
        )
        .await;
    assert_eq!(json.status, StatusCode::OK, "{}", json.body);
    assert!(json
        .header(header::CONTENT_TYPE)
        .starts_with("application/feed+json"));
    let document: Value = serde_json::from_str(&json.body).expect("json feed parses");
    assert_eq!(document["version"], "https://jsonfeed.org/version/1.1");
    assert!(document["items"].is_array());

    let rss = client
        .raw(
            "GET",
            &format!("/api/v1/saved-views/{id}/feed?format=rss"),
            None,
            &[],
        )
        .await;
    assert_eq!(rss.status, StatusCode::OK, "{}", rss.body);
    assert!(rss
        .header(header::CONTENT_TYPE)
        .starts_with("application/rss+xml"));
    assert!(rss.body.contains("<rss version=\"2.0\""));
    // The three shapes are three documents, so they do not share a tag.
    assert_ne!(rss.header(header::ETAG), tag);
}

#[tokio::test]
async fn a_token_in_the_query_reaches_the_same_scope_check_a_header_does() {
    let harness = Harness::new("token").await;
    let mut client = reader(&harness, "tokens").await;
    let id = save_catalogue(&mut client, "tokens").await;

    // No cookie: a feed reader has neither a session nor a header field.
    let mut anonymouse = harness.client();
    let bare = anonymouse
        .raw("GET", &format!("/api/v1/saved-views/{id}/feed"), None, &[])
        .await;
    assert_eq!(bare.status, StatusCode::UNAUTHORIZED, "{}", bare.body);

    let wrong = anonymouse
        .raw(
            "GET",
            &format!("/api/v1/saved-views/{id}/feed?token=not-a-real-token"),
            None,
            &[],
        )
        .await;
    assert_eq!(
        wrong.status,
        StatusCode::UNAUTHORIZED,
        "a bad token in the query is a bad token, not a 500: {}",
        wrong.body
    );
}

#[tokio::test]
async fn the_feed_belongs_to_the_account_and_an_unknown_shape_is_named() {
    let harness = Harness::new("scope").await;
    let mut owner = reader(&harness, "owner").await;
    let id = save_catalogue(&mut owner, "mine").await;

    let mut stranger = reader(&harness, "stranger").await;
    let theirs = stranger
        .raw("GET", &format!("/api/v1/saved-views/{id}/feed"), None, &[])
        .await;
    assert_eq!(
        theirs.status,
        StatusCode::NOT_FOUND,
        "someone else's view is absent, not forbidden: {}",
        theirs.body
    );

    let missing = owner
        .raw(
            "GET",
            "/api/v1/saved-views/00000000-0000-0000-0000-000000000000/feed",
            None,
            &[],
        )
        .await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND, "{}", missing.body);

    let unknown = owner
        .raw(
            "GET",
            &format!("/api/v1/saved-views/{id}/feed?format=yaml"),
            None,
            &[],
        )
        .await;
    assert_eq!(
        unknown.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        unknown.body
    );
    assert!(
        unknown.body.contains("atom"),
        "the answer names the shapes: {}",
        unknown.body
    );
}
