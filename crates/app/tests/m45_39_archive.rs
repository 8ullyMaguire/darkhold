//! M45-39 phase 2 — §13's account archive over HTTP.
//!
//! What only the route can get wrong:
//!
//! 1. **It is the account's own archive.** There is no id parameter, and a session's archive contains
//!    that account's rows and nobody else's.
//! 2. **The manifest is a real checksum.** The test recomputes each section's SHA-256 from the returned
//!    rows; a manifest that echoed a constant would pass a count check and fail this.
//! 3. **It arrives as a file**, with `attachment` and `no-store`.
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
use sha2::{Digest, Sha256};
use test_support::TestDb;
use tower::ServiceExt;

const PASSWORD: &str = "test-pass-1234";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-archive-{tag}-{}-{:?}",
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
    ) -> (StatusCode, Value, axum::http::HeaderMap) {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json");
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
        let headers = response.headers().clone();
        self.capture(&response);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, json, headers)
    }

    async fn get(&mut self, uri: &str) -> (StatusCode, Value) {
        let (status, body, _) = self.request("GET", uri, None).await;
        (status, body)
    }

    async fn send(&mut self, method: &str, uri: &str, body: Value) -> (StatusCode, Value) {
        let (status, response, _) = self.request(method, uri, Some(body)).await;
        (status, response)
    }

    async fn register(&mut self, handle: &str) -> String {
        let (status, body) = self
            .send(
                "POST",
                "/api/v1/auth/register",
                json!({
                    "email": format!("archive-{handle}@example.test"),
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

/// Recompute a section's hash from the rows the response carried.
fn hash_of(rows: &Value) -> String {
    let body = serde_json::to_string(rows).expect("section json");
    hex::encode(Sha256::digest(body.as_bytes()))
}

#[tokio::test]
async fn an_archive_needs_a_session() {
    let h = Harness::new("anon").await;
    let mut stranger = h.client();
    let (status, _) = stranger.get("/api/v1/me/archive").await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "an archive of somebody's account is not a public document"
    );
}

#[tokio::test]
async fn the_archive_is_a_file_with_real_checksums() {
    let h = Harness::new("checksums").await;
    let mut reader = h.client();
    reader.register("reader").await;

    let (status, body, headers) = reader.request("GET", "/api/v1/me/archive", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    assert_eq!(body["schema"], json!("darkhold-archive/1"));
    assert!(body["generated_at"].as_str().is_some(), "{body}");

    let disposition = headers
        .get(header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    assert!(
        disposition.starts_with("attachment;"),
        "an archive arrives as a file: {disposition}"
    );
    assert!(
        disposition.contains("darkhold-archive-"),
        "and says what it is: {disposition}"
    );
    assert!(
        !disposition.contains("reader"),
        "the filename never names the account"
    );

    // Every declared section is present, and each manifest hash is the hash of its own rows.
    let manifest = body["manifest"].as_array().expect("manifest");
    assert!(
        manifest.len() >= 10,
        "the archive is not one table: {manifest:?}"
    );
    for entry in manifest {
        let name = entry["name"].as_str().expect("section name");
        let rows = &body[name];
        assert!(
            rows.is_array(),
            "section {name} is in the manifest but not the document"
        );
        assert_eq!(
            entry["count"].as_u64().unwrap_or(999),
            rows.as_array().unwrap().len() as u64,
            "count and rows disagree for {name}"
        );
        assert_eq!(
            entry["sha256"].as_str().unwrap_or_default(),
            hash_of(rows),
            "the manifest hash for {name} is not the hash of its rows"
        );
    }

    // The account section is this reader's own.
    let account = body["account"].as_array().expect("account rows");
    assert_eq!(account.len(), 1);
    assert_eq!(account[0]["email"], json!("archive-reader@example.test"));
    assert!(
        account[0].get("email_verified_at").is_none(),
        "the archive carries what it declares and nothing it does not"
    );

    assert_eq!(body["pseuds"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn the_archive_holds_this_accounts_rows_and_no_others() {
    let h = Harness::new("isolation").await;
    let mut alice = h.client();
    alice.register("alice").await;

    // Alice writes something and saves a work to her shelf.
    let (status, body) = alice
        .send("POST", "/api/v1/works", json!({ "title": "Alice's Work" }))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let work_id = body["id"].as_str().expect("work id").to_owned();

    let mut bob = h.client();
    bob.register("bob").await;
    // The bookmark door is the library's, keyed by subject rather than nested under the work.
    let (status, body) = bob
        .send(
            "POST",
            "/api/v1/bookmarks",
            json!({
                "subject_type": "work",
                "subject_id": work_id,
                "note": "bob was here",
                "is_public": false,
            }),
        )
        .await;
    assert!(
        status == StatusCode::CREATED || status == StatusCode::OK,
        "bob bookmarks alice's work: {body}"
    );

    let (status, alice_archive) = alice.get("/api/v1/me/archive").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        alice_archive["works"].as_array().unwrap().len(),
        1,
        "alice's archive holds her work: {alice_archive}"
    );
    assert!(
        alice_archive["bookmarks"].as_array().unwrap().is_empty(),
        "and not bob's bookmark: {alice_archive}"
    );

    let (status, bob_archive) = bob.get("/api/v1/me/archive").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        bob_archive["works"].as_array().unwrap().is_empty(),
        "bob's archive holds no works: {bob_archive}"
    );
    let bob_bookmarks = bob_archive["bookmarks"].as_array().unwrap();
    assert_eq!(
        bob_bookmarks.len(),
        1,
        "his own bookmark is his: {bob_archive}"
    );
    assert_eq!(bob_bookmarks[0]["note"], json!("bob was here"));
}
