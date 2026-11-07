//! M71-09 through the real router: three states, three sentences, and not one fabricated percentage.
//!
//! The domain tests prove the decision function. This proves the wiring a unit test cannot see: `GET /api/v1/reading/progress` joins each
//! stored position to the revision its unit points at **now**, and answers `percent: null` with `display: "—"` whenever there is no honest
//! number. The failure this guards is the one every reading app has: a resume banner that says "42%" about a chapter rewritten since, or
//! "0%" about a place nobody measured.
//!
//! The three cases are produced the way a reader produces them:
//! 1. save a position, read it back — `current`, `42%`;
//! 2. save a position, then edit the chapter (appending a revision) — `stale`, `—`, both places offered;
//! 3. two devices at different places — `conflict`, `—`, the reader chooses.
//!
//! A fourth case — a row with no revision, no anchor and no movement — cannot arise from the save route (it always records the revision it
//! saw), so it is proved in `darkhold-domain`'s unit tests rather than faked here.

use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use serde_json::{json, Value};
use test_support::TestDb;
use tower::ServiceExt;

const PASSWORD: &str = "test-pass-1234";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-m7109-{tag}-{}-{:?}",
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
    config
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
            Some(value) => Body::from(value.to_string()),
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
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn get(&mut self, uri: &str) -> (StatusCode, Value) {
        self.request("GET", uri, None).await
    }

    async fn post(&mut self, uri: &str, body: Value) -> (StatusCode, Value) {
        self.request("POST", uri, Some(body)).await
    }

    async fn put(&mut self, uri: &str, body: Value) -> (StatusCode, Value) {
        self.request("PUT", uri, Some(body)).await
    }

    async fn patch(&mut self, uri: &str, body: Value) -> (StatusCode, Value) {
        self.request("PATCH", uri, Some(body)).await
    }

    async fn register(&mut self, handle: &str) {
        let (status, body) = self
            .post(
                "/api/v1/auth/register",
                json!({
                    "email": format!("m71-09-{handle}@example.test"),
                    "password": PASSWORD,
                    "handle": handle,
                    "display_name": handle,
                    "age_band": "adult",
                }),
            )
            .await;
        assert!(
            status == StatusCode::CREATED || status == StatusCode::OK,
            "register {handle}: {body}"
        );
    }
}

/// A work with one chapter, created through the routes so every id is real.
async fn story(client: &mut Client) -> (String, String) {
    let (status, work) = client
        .post("/api/v1/works", json!({ "title": "A Story" }))
        .await;
    assert!(status.is_success(), "create the work: {work}");
    let work_id = work["id"].as_str().expect("the work id").to_owned();
    let (status, chapter) = client
        .post(
            &format!("/api/v1/works/{work_id}/chapters"),
            json!({ "title": "Chapter One" }),
        )
        .await;
    assert!(status.is_success(), "create the chapter: {chapter}");
    let chapter_id = chapter["id"].as_str().expect("the chapter id").to_owned();
    // A title alone leaves the unit with no revision, and a position with no revision is a place nothing can prove stale. Writing the body
    // is what gives the chapter a first revision — the state any real reader arrives at.
    let (status, written) = client
        .patch(
            &format!("/api/v1/chapters/{chapter_id}"),
            json!({
                "document": {"type": "doc", "content": [
                    {"type": "paragraph", "content": [{"type": "text", "text": "Once upon a time."}]}
                ]},
                "expected_version": 1,
            }),
        )
        .await;
    assert!(status.is_success(), "write the chapter: {written}");
    (work_id, chapter_id)
}

fn progress_uri(work_id: &str) -> String {
    format!("/api/v1/reading/progress?subject_type=work&subject_id={work_id}")
}

#[tokio::test]
async fn a_position_saved_against_the_current_revision_is_the_one_percentage() {
    let h = Harness::new("current").await;
    let mut reader = h.client();
    reader.register("reader").await;
    let (work_id, chapter_id) = story(&mut reader).await;

    let (status, body) = reader
        .put(
            "/api/v1/reading/progress",
            json!({
                "subject_type": "work",
                "subject_id": work_id,
                "unit_id": chapter_id,
                "position_permille": 420,
                "device_id": "laptop",
            }),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "save the position: {body}");

    let (status, body) = reader.get(&progress_uri(&work_id)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let resume = &body["resume"];
    assert_eq!(resume["state"], "current", "{resume}");
    assert_eq!(resume["percent"], 420, "{resume}");
    assert_eq!(resume["display"], "42%", "{resume}");
    assert_eq!(resume["needs_a_choice"], false);
}

#[tokio::test]
async fn a_position_from_before_the_text_changed_is_stale_and_offers_both_places() {
    let h = Harness::new("stale").await;
    let mut author = h.client();
    author.register("author").await;
    let (work_id, chapter_id) = story(&mut author).await;

    // The author reads what they wrote and puts the place down.
    let (status, body) = author
        .put(
            "/api/v1/reading/progress",
            json!({
                "subject_type": "work",
                "subject_id": work_id,
                "unit_id": chapter_id,
                "position_permille": 420,
                "device_id": "laptop",
            }),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (_, before) = author.get(&progress_uri(&work_id)).await;
    assert_eq!(before["resume"]["state"], "current", "{before}");

    // Then they rewrite the chapter: a new revision, and the saved place is no longer a percentage of the text.
    let (status, edited) = author
        .patch(
            &format!("/api/v1/chapters/{chapter_id}"),
            json!({
                "document": {"type": "doc", "content": [
                    {"type": "paragraph", "content": [{"type": "text", "text": "Rewritten."}]}
                ]},
                "expected_version": 2,
            }),
        )
        .await;
    assert!(status.is_success(), "rewrite the chapter: {edited}");

    let (status, body) = author.get(&progress_uri(&work_id)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let resume = &body["resume"];
    assert_eq!(resume["state"], "stale", "{body}");
    assert!(
        resume["percent"].is_null(),
        "the permille is null — not 420 and not 0: {resume}"
    );
    assert_eq!(resume["display"], "—", "{resume}");
    assert_eq!(resume["needs_a_choice"], true);
    let sentence = resume["sentence"].as_str().expect("sentence");
    assert!(
        sentence.contains("earlier version") && sentence.contains("start from the top"),
        "both places are offered in words: {sentence}"
    );
    // The raw row still travels: the reader may choose it. The honesty is about the claim, not about hiding the position.
    assert_eq!(body["positions"][0]["position_permille"], 420, "{body}");
}

#[tokio::test]
async fn two_devices_that_disagree_ask_the_reader_and_show_no_single_percentage() {
    let h = Harness::new("conflict").await;
    let mut reader = h.client();
    reader.register("reader").await;
    let (work_id, chapter_id) = story(&mut reader).await;

    for (device, permille) in [("laptop", 100), ("phone", 900)] {
        let (status, body) = reader
            .put(
                "/api/v1/reading/progress",
                json!({
                    "subject_type": "work",
                    "subject_id": work_id,
                    "unit_id": chapter_id,
                    "position_permille": permille,
                    "device_id": device,
                }),
            )
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "save {device}: {body}");
    }

    let (status, body) = reader.get(&progress_uri(&work_id)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let resume = &body["resume"];
    assert_eq!(resume["state"], "conflict", "{body}");
    assert!(
        resume["percent"].is_null(),
        "two places, no basis: {resume}"
    );
    assert_eq!(resume["display"], "—");
    assert_eq!(resume["needs_a_choice"], true);
    assert!(
        body["resume"]["sentence"]
            .as_str()
            .unwrap_or_default()
            .contains("Two devices"),
        "{body}"
    );
    assert_eq!(
        body["positions"].as_array().map(Vec::len),
        Some(2),
        "both places travel so the page can offer them: {body}"
    );
}
