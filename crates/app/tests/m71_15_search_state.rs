//! M71-15: the query string and the facet panel are one state.
//!
//! The row's premise, and the reason this file exists: the grammar had no way to say "no work above `mature`", so a search page that
//! presented the string as *the* state silently dropped the rating ceiling the moment anyone edited it — the filter stayed applied and
//! stopped being visible, which is the product's §8.14 failure ("named degradation") in its quietest form.
//!
//! `GET /api/v1/search` now answers with both halves of the state:
//!
//! ```text
//! "state": { "query": "...", "chips": [ {kind, value, label, editable_in_query, removable, reason?} ],
//!            "unexpressed_note": null | "1 of this search's filters is not written in the query: ..." }
//! ```
//!
//! Asserts:
//! 1. the word window and the rating ceiling are **in** the string, in canonical order, and the string parses back to them — the ceiling
//!    is the half that did not parse before `rating:<=mature` was added to the grammar;
//! 2. a reader's content filter is a chip that the string cannot carry: `editable_in_query: false`, `removable: false`, with the reason —
//!    so no page may offer a control that pretends editing the string removes it;
//! 3. `unexpressed_note` names that filter in words, and is absent when everything is expressible;
//! 4. the string is canonical: two requests that describe the same state produce the same string.

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
        "darkhold-m7115-{tag}-{}-{:?}",
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
        if !self.cookies.is_empty() {
            let header_value = self
                .cookies
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect::<Vec<String>>()
                .join("; ");
            builder = builder.header(header::COOKIE, header_value);
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
        let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, value)
    }

    async fn get(&mut self, uri: &str) -> (StatusCode, Value) {
        self.request("GET", uri, None).await
    }

    async fn post(&mut self, uri: &str, body: Value) -> (StatusCode, Value) {
        self.request("POST", uri, Some(body)).await
    }

    async fn register(&mut self, handle: &str) {
        let (status, body) = self
            .post(
                "/api/v1/auth/register",
                json!({
                    "email": format!("m71-15-{handle}@example.test"),
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

fn chip<'a>(state: &'a Value, kind: &str) -> &'a Value {
    state["chips"]
        .as_array()
        .expect("chips array")
        .iter()
        .find(|chip| chip["kind"] == kind)
        .unwrap_or_else(|| panic!("no {kind} chip in {state}"))
}

#[tokio::test]
async fn the_string_carries_the_word_window_and_the_rating_ceiling() {
    let h = Harness::new("string").await;
    let mut client = h.client();

    // Anonymous: explicit parameters are the caller's own request and apply without a session (M47-06b), which makes this the plainest
    // statement of the row: nothing here needs a reader for the string to be complete.
    let (status, body) = client
        .get("/api/v1/search?q=slow%20burn&min_words=1000&max_rating=mature")
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let state = &body["state"];
    let query = state["query"].as_str().expect("state.query");
    assert_eq!(
        query, "\"slow burn\" words:>=1000 rating:<=mature",
        "canonical order: text, word window, ceiling"
    );

    // The half that did not parse before this row: a ceiling is a rank comparison, and the grammar had no ranked kind.
    let parsed = darkhold_domain::filter_state::FilterState::from_query_string(query);
    assert_eq!(parsed.text, "slow burn");
    assert_eq!(parsed.min_words, Some(1_000));
    assert_eq!(
        parsed.max_rating.as_deref(),
        Some("mature"),
        "the ceiling survives the string, which is the whole point"
    );

    // Each expressible filter is a chip the string owns.
    for kind in ["min_words", "max_rating"] {
        let chip = chip(state, kind);
        assert_eq!(chip["editable_in_query"], true, "{chip}");
        assert_eq!(chip["removable"], true, "{chip}");
        assert!(chip["reason"].is_null(), "{chip}");
    }
    assert!(
        state["unexpressed_note"].is_null(),
        "nothing to apologise for: {state}"
    );
}

#[tokio::test]
async fn the_string_is_canonical_so_two_equal_searches_share_one_url() {
    let h = Harness::new("canonical").await;
    let mut client = h.client();

    let (_, first) = client
        .get("/api/v1/search?q=winter&max_words=5000&min_words=100")
        .await;
    let (_, second) = client
        .get("/api/v1/search?min_words=100&q=winter&max_words=5000")
        .await;
    assert_eq!(
        first["state"]["query"], second["state"]["query"],
        "the same state is one string, whatever order the parameters arrived in"
    );
    assert_eq!(
        first["state"]["query"].as_str().unwrap_or_default(),
        "winter words:>=100 words:<=5000"
    );
}

#[tokio::test]
async fn a_content_filter_is_a_pinned_chip_the_string_cannot_carry() {
    let h = Harness::new("pinned").await;
    let mut client = h.client();
    client.register("reader").await;

    let (status, body) = client
        .post(
            "/api/v1/settings/content-filters",
            json!({ "filter_type": "warning", "value": "graphic violence" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = client.get("/api/v1/search?q=winter&max_rating=teen").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let state = &body["state"];

    let filter = chip(state, "warning");
    assert_eq!(filter["value"], "graphic violence", "{filter}");
    assert_eq!(
        filter["editable_in_query"], false,
        "the grammar has no field for a content filter: {filter}"
    );
    assert_eq!(
        filter["removable"], false,
        "a chip the string cannot express must not be droppable by editing it: {filter}"
    );
    assert!(
        filter["reason"]
            .as_str()
            .unwrap_or_default()
            .contains("no field"),
        "it says why: {filter}"
    );

    // The note names it in words, so a page does not have to invent the sentence.
    let note = state["unexpressed_note"].as_str().unwrap_or_default();
    assert!(
        note.contains("1 of this search's filters is not written in the query")
            && note.contains("graphic violence"),
        "the note names the filter: {note:?}"
    );

    // …and the ceiling is still in the string beside it: the two halves are one state, not two.
    assert_eq!(
        state["query"].as_str().unwrap_or_default(),
        "winter rating:<=teen",
        "{state}"
    );
}

#[tokio::test]
async fn an_unknown_rating_in_a_comparison_is_refused_by_name() {
    let h = Harness::new("refusal").await;
    let mut client = h.client();

    let (status, body) = client.get("/api/v1/search?q=rating:%3C=smut").await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a rank the vocabulary does not contain is a refusal, not an empty result: {body}"
    );
    let message = body["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("rating") && message.contains("general"),
        "the refusal names the field and the vocabulary: {message}"
    );
}
