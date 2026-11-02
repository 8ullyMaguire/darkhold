//! M45-38 — author-note links are monitored, over HTTP (§32.7, gaps review D3).
//!
//! What only the route and the save path can get wrong:
//!
//! 1. **Saving a chapter with a note registers its links**, with no second action from the author —
//!    a monitor nobody has to switch on is the only kind that watches anything.
//! 2. **A note with no link registers nothing**, so the common case costs nothing.
//! 3. **The report is about the live revision.** Correcting a note in a new revision replaces what
//!    the work says; the old row survives for evidence but stops being reported.
//! 4. **A reader can read the report and only the author can trigger a check.** The reader is the
//!    person about to click.
//! 5. **A refused fetch is recorded as dead, not left pending.** The check runs through the same SSRF
//!    guard as a media fetch, so a link to `127.0.0.1` is refused before any connection — and the
//!    test asserts on that refusal rather than on the network, which makes it deterministic.
//!
//! Both engines: `TestDb` is SQLite by default and PostgreSQL under `DARKHOLD_TEST_PG_URL`.

//! M47-14 — §9.7.6's debut badge (audit item 75) over HTTP.
//!
//! What only the route and the publish path can get wrong:
//!
//! 1. **A first publication awards First Words, with its 20 credits**, on the author's *account*
//!    rather than their pseud: a reader who adds a second identity does not get a second badge.
//! 2. **A second publication awards nothing** — the badge is one-time, the ledger is not paid twice,
//!    and the profile stops calling the author new.
//! 3. **"Debut" is derived**, so it stops being true when the second work lands rather than when
//!    somebody remembers to clear a flag.
//!
//! The badge is written AFTER the publication commits, so a badge or ledger failure cannot roll back
//! a publication the author was already told succeeded.
//!
//! Both engines: `TestDb` is SQLite by default and PostgreSQL under `DARKHOLD_TEST_PG_URL`.

use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::DatabaseConfig;
use serde_json::{json, Value};
use test_support::TestDb;
use tower::ServiceExt;

const PASSWORD: &str = "test-pass-1234";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-m45-38-{tag}-{}-{:?}",
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

    fn cookie_header(&self) -> Option<String> {
        if self.cookies.is_empty() {
            return None;
        }
        Some(
            self.cookies
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect::<Vec<String>>()
                .join("; "),
        )
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
        if let Some(cookie) = self.cookie_header() {
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
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, json)
    }

    async fn get(&mut self, uri: &str) -> (StatusCode, Value) {
        self.request("GET", uri, None).await
    }

    async fn post(&mut self, uri: &str, body: Value) -> (StatusCode, Value) {
        self.request("POST", uri, Some(body)).await
    }

    async fn patch(&mut self, uri: &str, body: Value) -> (StatusCode, Value) {
        self.request("PATCH", uri, Some(body)).await
    }

    async fn register(&mut self, handle: &str) -> String {
        let (status, body) = self
            .post(
                "/api/v1/auth/register",
                json!({
                    "email": format!("m45-38-{handle}@example.test"),
                    "password": PASSWORD,
                    "handle": handle,
                    "display_name": handle,
                    "age_band": "adult",
                }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "register {handle}: {body}");
        body["pseuds"][0]["handle"]
            .as_str()
            .unwrap_or(handle)
            .to_owned()
    }

    /// Create a work with one written chapter and publish it. Returns the work id.
    async fn publish_a_work(&mut self, title: &str) -> String {
        let (status, body) = self.post("/api/v1/works", json!({ "title": title })).await;
        assert_eq!(status, StatusCode::CREATED, "create {title}: {body}");
        let work_id = body["id"].as_str().expect("work id").to_owned();
        let work_version = body["version"].as_i64().expect("work version");

        let (status, body) = self
            .post(
                &format!("/api/v1/works/{work_id}/chapters"),
                json!({ "title": "Chapter One" }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "create the chapter: {body}");
        let unit_id = body["id"].as_str().expect("unit id").to_owned();
        let unit_version = body["version"].as_i64().expect("unit version");

        let document = json!({ "type": "doc", "content": [
            { "type": "paragraph", "content": [{ "type": "text", "text": "Something worth reading." }] },
        ] });
        let (status, body) = self
            .patch(
                &format!("/api/v1/chapters/{unit_id}"),
                json!({
                    "expected_version": unit_version,
                    "document": document,
                    // The note carries two links, one wrapped in prose punctuation: this helper is
                    // what the first test asserts the extraction of.
                    "note": "(see https://example.test/meta). Also https://blog.tumblr.com/post/1, thanks!"
                }),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "write the chapter: {body}");

        let (status, body) = self
            .post(
                &format!("/api/v1/works/{work_id}/publish"),
                json!({ "expected_version": work_version, "idempotency_key": format!("m45-38-{work_id}") }),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "publish {title}: {body}");
        work_id
    }
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

#[tokio::test]
async fn saving_a_note_registers_its_links_and_the_report_lists_them() {
    let harness = Harness::new("registers").await;
    let mut author = harness.client();
    author.register("nl-author").await;
    let work = author.publish_a_work("Note Links").await;

    let (status, body) = author
        .get(&format!("/api/v1/works/{work}/note-links"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["count"].as_u64(),
        Some(2),
        "the published chapter's note carried two links: {body}"
    );
    assert_eq!(body["dead"].as_u64(), Some(0));
    let hosts: Vec<&str> = body["links"]
        .as_array()
        .expect("links")
        .iter()
        .filter_map(|link| link["host"].as_str())
        .collect();
    assert!(hosts.contains(&"example.test"), "{hosts:?}");
    assert!(hosts.contains(&"blog.tumblr.com"), "{hosts:?}");
    let tumblr = body["links"]
        .as_array()
        .expect("links")
        .iter()
        .find(|link| link["host"] == "blog.tumblr.com")
        .expect("the tumblr link");
    assert_eq!(tumblr["provider"].as_str(), Some("tumblr"));
    assert_eq!(tumblr["status"].as_str(), Some("pending_verification"));
}

#[tokio::test]
async fn a_note_without_links_reports_nothing() {
    let harness = Harness::new("none").await;
    let mut author = harness.client();
    author.register("nl-quiet").await;

    let (status, body) = author
        .post("/api/v1/works", json!({ "title": "No Links" }))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let work = body["id"].as_str().expect("work").to_owned();
    let work_version = body["version"].as_i64().expect("version");
    let (status, body) = author
        .post(
            &format!("/api/v1/works/{work}/chapters"),
            json!({ "title": "Chapter One" }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let unit = body["id"].as_str().expect("unit").to_owned();
    let unit_version = body["version"].as_i64().expect("version");
    let document = json!({ "type": "doc", "content": [
        { "type": "paragraph", "content": [{ "type": "text", "text": "Just prose, no links." }] },
    ] });
    let (status, body) = author
        .patch(
            &format!("/api/v1/chapters/{unit}"),
            json!({ "expected_version": unit_version, "document": document, "note": "Thanks to nobody." }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = author
        .post(
            &format!("/api/v1/works/{work}/publish"),
            json!({ "expected_version": work_version, "idempotency_key": format!("m45-38-{work}") }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = author
        .get(&format!("/api/v1/works/{work}/note-links"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["count"].as_u64(), Some(0), "no links, no rows: {body}");
}

#[tokio::test]
async fn a_corrected_note_replaces_what_the_work_reports() {
    let harness = Harness::new("corrected").await;
    let mut author = harness.client();
    author.register("nl-corrector").await;

    let (status, body) = author
        .post("/api/v1/works", json!({ "title": "Corrected" }))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let work = body["id"].as_str().expect("work").to_owned();
    let (status, body) = author
        .post(
            &format!("/api/v1/works/{work}/chapters"),
            json!({ "title": "Chapter One" }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let unit = body["id"].as_str().expect("unit").to_owned();
    // First save: one link.
    let version = body["version"].as_i64().expect("version");
    let document = json!({ "type": "doc", "content": [
        { "type": "paragraph", "content": [{ "type": "text", "text": "Body." }] },
    ] });
    let (status, body) = author
        .patch(
            &format!("/api/v1/chapters/{unit}"),
            json!({
                "expected_version": version,
                "document": document,
                "note": "Old pointer: https://old.test/x"
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let version = body["version"].as_i64().expect("version");

    // Second save: the note is corrected to point somewhere else.
    let (status, body) = author
        .patch(
            &format!("/api/v1/chapters/{unit}"),
            json!({
                "expected_version": version,
                "document": document,
                "note": "Fixed pointer: https://new.test/x"
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = author
        .get(&format!("/api/v1/works/{work}/note-links"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["count"].as_u64(),
        Some(1),
        "only the live revision's note is reported: {body}"
    );
    assert_eq!(body["links"][0]["host"].as_str(), Some("new.test"));

    // Both rows are still stored: the old one is evidence, not debris.
    let stored = darkhold_db::note_links::list_for_work(harness.tdb.db(), &work)
        .await
        .expect("store");
    assert_eq!(
        stored.len(),
        2,
        "the superseded revision's link is still stored"
    );
}

#[tokio::test]
async fn a_stranger_can_read_the_report_but_only_the_author_can_trigger_a_check() {
    let harness = Harness::new("authz").await;
    let mut author = harness.client();
    author.register("nl-owner").await;
    let work = author.publish_a_work("Owned Notes").await;
    let mut stranger = harness.client();
    stranger.register("nl-stranger").await;

    let (status, body) = stranger
        .get(&format!("/api/v1/works/{work}/note-links"))
        .await;
    assert_eq!(status, StatusCode::OK, "the report is public: {body}");
    assert_eq!(body["count"].as_u64(), Some(2));

    let (status, _body) = stranger
        .post(&format!("/api/v1/works/{work}/note-links/check"), json!({}))
        .await;
    assert!(
        status == StatusCode::NOT_FOUND || status == StatusCode::FORBIDDEN,
        "a stranger may not spend the instance's outbound requests: {status}"
    );
}

#[tokio::test]
async fn a_refused_url_is_recorded_dead_rather_than_left_pending() {
    let harness = Harness::new("refused").await;
    let mut author = harness.client();
    author.register("nl-loopback").await;

    // The note points at loopback, which the fetch guard refuses unconditionally. That refusal is the
    // test's subject: it is deterministic, needs no network, and is exactly what a stored link to a
    // private address must produce.
    let (status, body) = author
        .post("/api/v1/works", json!({ "title": "Loopback Note" }))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let work = body["id"].as_str().expect("work").to_owned();
    let work_version = body["version"].as_i64().expect("version");
    let (status, body) = author
        .post(
            &format!("/api/v1/works/{work}/chapters"),
            json!({ "title": "Chapter One" }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let unit = body["id"].as_str().expect("unit").to_owned();
    let unit_version = body["version"].as_i64().expect("version");
    let document = json!({ "type": "doc", "content": [
        { "type": "paragraph", "content": [{ "type": "text", "text": "See." }] },
    ] });
    let (status, body) = author
        .patch(
            &format!("/api/v1/chapters/{unit}"),
            json!({
                "expected_version": unit_version,
                "document": document,
                "note": "Local mirror: http://127.0.0.1:9/x"
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = author
        .post(
            &format!("/api/v1/works/{work}/publish"),
            json!({ "expected_version": work_version, "idempotency_key": format!("m45-38-loop-{work}") }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = author
        .post(&format!("/api/v1/works/{work}/note-links/check"), json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["dead"].as_u64(), Some(1), "{body}");

    let (status, body) = author
        .get(&format!("/api/v1/works/{work}/note-links"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["links"][0]["status"].as_str(),
        Some("dead"),
        "the refusal is recorded, with a reason: {body}"
    );
    assert!(
        body["links"][0]["failure_reason"].as_str().is_some(),
        "dead without a why is a verdict the author cannot act on: {body}"
    );
}
