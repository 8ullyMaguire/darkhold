//! M69-03 — passports through the doors: export a signed document, present it elsewhere, and withdraw it.
//!
//! The interesting half is the *pair*: the document the export door emits must be one the import door accepts — signed by
//! the key the instance configured, over the bytes the receiver canonicalises. A test that only checked "export returns 200"
//! would pass on two implementations that disagree about the signed surface, which is precisely the failure a signature is
//! supposed to prevent.
//!
//! So the test takes the exported JSON, hands it straight back to `POST /me/passport` as a different reader, and asserts the
//! receiving side's own verification accepted it and seeded taste. Then it tampers with one weight and asserts the same door
//! refuses it.

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
/// A fixed 32-byte key, so the test is reproducible.
const KEY: &str = "2222222222222222222222222222222222222222222222222222222222222222";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-m6903-{tag}-{}-{:?}",
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
        Self { app, cookies: Vec::new() }
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
                    "email": format!("m69-03-{handle}@example.test"),
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
        let (_, me) = self.send("GET", "/api/v1/auth/me", None).await;
        me["account"]["id"]
            .as_str()
            .or_else(|| me["id"].as_str())
            .expect("account id")
            .to_owned()
    }
}

fn router(tdb: &TestDb, dir: &std::path::Path, open: bool, key: Option<&str>) -> axum::Router {
    let mut config = Config::development_defaults();
    config.storage.root = dir.to_path_buf();
    config.discovery.taste_passports = open;
    config.discovery.passport_key = key.map(|text| text.to_owned());
    server::build_router(AppState::new(config, tdb.db().clone()))
}

#[tokio::test]
async fn an_exported_passport_is_accepted_by_the_import_door_and_a_tampered_one_is_not() {
    server::set_trust_proxy(false);
    let dir = scratch_dir("passport");
    let tdb = TestDb::connect_with_dir("m69-03-passport", &dir).await;

    let mut reader = Client::new(router(&tdb, &dir, true, Some(KEY)));
    let reader_handle = "passport-exporter";
    let reader_id = reader.register(reader_handle).await;

    // Something for the passport to describe: two bookmarks carrying tags.
    seed_shelved(&tdb, &reader_id, "The loved one", &["angst", "slowburn"]).await;
    seed_shelved(&tdb, &reader_id, "Another", &["angst"]).await;

    let (status, body) = reader.send("GET", "/api/v1/me/passport", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let signed = body["passport"].clone();
    let public = darkhold_domain::passport::public_key_hex(KEY).expect("public key");
    assert_eq!(
        signed["key"],
        json!(public),
        "the document carries the PUBLIC half of this instance's key -- never the seed"
    );
    assert_eq!(signed["passport"]["subject"], json!(reader_handle), "{body}");
    assert_eq!(signed["passport"]["version"], json!(1));
    let taste = signed["passport"]["taste"].as_object().expect("taste");
    assert_eq!(taste.len(), 2, "two tags, normalised: {body}");
    assert_eq!(taste["angst"], json!(6667), "two of three tag occurrences, in basis points: {body}");
    assert_eq!(taste["slowburn"], json!(3333), "{body}");
    // The two works the test shelved were created under the reader's own pseud, so the count is theirs -- which is the
    // point of counting: a passport says how much a reader has done, not what they did.
    assert_eq!(signed["passport"]["contributions"]["works"], json!(2), "{body}");
    assert_eq!(signed["passport"]["contributions"]["comments"], json!(0), "{body}");

    // A second reader presents the document verbatim: the receiving side verifies it and seeds imported taste.
    let mut receiver = Client::new(router(&tdb, &dir, true, Some(KEY)));
    let receiver_id = receiver.register("passport-receiver").await;
    let (status, body) = receiver
        .send("POST", "/api/v1/me/passport", Some(json!({ "passport": signed })))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["signals"], json!(2), "{body}");
    assert_eq!(body["origin"], json!("http://localhost:8080"), "{body}");

    let (status, body) = receiver.send("GET", "/api/v1/me/passports", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["passports"].as_array().expect("receipts").len(), 1, "{body}");
    assert_eq!(body["passports"][0]["signals"], json!(2), "{body}");

    let imported = imported_signals(&tdb, &receiver_id).await;
    assert_eq!(imported.len(), 2, "{imported:?}");
    assert!(
        imported.iter().all(|origin| origin == "imported"),
        "the receiving instance marks it as somebody else's measurement: {imported:?}"
    );

    // Tampering with one weight: same door, refused, and nothing added.
    let mut forged = signed.clone();
    forged["passport"]["taste"]["angst"] = json!(9_999);
    let (status, _) = receiver
        .send("POST", "/api/v1/me/passport", Some(json!({ "passport": forged })))
        .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a document whose bytes no longer match its signature is refused"
    );
    assert_eq!(imported_signals(&tdb, &receiver_id).await.len(), 2, "and nothing was added");

    // Withdrawing takes the receipt and the taste with it.
    let (status, body) = receiver
        .send(
            "DELETE",
            "/api/v1/me/passport?origin=http%3A%2F%2Flocalhost%3A8080",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(imported_signals(&tdb, &receiver_id).await.is_empty());
    let (_, body) = receiver.send("GET", "/api/v1/me/passports", None).await;
    assert!(body["passports"].as_array().expect("receipts").is_empty(), "{body}");
}

#[tokio::test]
async fn export_without_a_configured_key_is_refused_with_the_reason_and_the_doors_are_shut_by_default() {
    server::set_trust_proxy(false);
    let dir = scratch_dir("passport-closed");
    let tdb = TestDb::connect_with_dir("m69-03-closed", &dir).await;

    // Open, but no key: the export door says why rather than signing with something nobody configured.
    let mut reader = Client::new(router(&tdb, &dir, true, None));
    let _ = reader.register("no-key-reader").await;
    let (status, body) = reader.send("GET", "/api/v1/me/passport", None).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .expect("message")
            .contains("passport_key"),
        "the refusal names the setting: {body}"
    );

    // Closed: every passport door is a 404, including the ones that would only read.
    let mut shut = Client::new(router(&tdb, &dir, false, Some(KEY)));
    let _ = shut.register("shut-reader").await;
    for (method, path) in [
        ("GET", "/api/v1/me/passport"),
        ("GET", "/api/v1/me/passports"),
        ("DELETE", "/api/v1/me/passport?origin=https%3A%2F%2Fx.example"),
    ] {
        let (status, _) = shut.send(method, path, None).await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "{method} {path} must be shut by default: a passport carries a reader's taste off this instance"
        );
    }
}

async fn seed_shelved(tdb: &TestDb, account: &str, title: &str, tags: &[&str]) {
    let now = "2026-11-07T00:00:00Z";
    let pseud: String = sqlx::query_scalar("SELECT id FROM pseuds ORDER BY created_at LIMIT 1")
        .fetch_one(tdb.db().sqlite_pool().expect("sqlite"))
        .await
        .expect("a pseud exists");
    let owner: darkhold_domain::ids::PseudId = pseud.parse().expect("pseud id");
    let work = darkhold_db::content::create_work(tdb.db(), owner, title, None)
        .await
        .expect("create work");
    let work_id = work.id.to_string();
    for tag in tags {
        // The passport reads the tag's canonical name, so the node has to exist as a taxonomy node: a bare node id in
        // ``work_tags` is an id nothing can name.
        sqlx::query(
            "INSERT OR IGNORE INTO canonical_entities (id, kind, canonical, norm, created_at)
             VALUES (?, 'tag', ?, ?, ?)",
        )
        .bind(format!("node-{tag}"))
        .bind(tag)
        .bind(tag)
        .bind(now)
        .execute(tdb.db().sqlite_pool().expect("sqlite"))
        .await
        .expect("canonical entity");
        sqlx::query("INSERT OR IGNORE INTO work_tags (work_id, node_id, added_at) VALUES (?, ?, ?)")
            .bind(&work_id)
            .bind(format!("node-{tag}"))
            .bind(now)
            .execute(tdb.db().sqlite_pool().expect("sqlite"))
            .await
            .expect("tag");
    }
    sqlx::query(
        "INSERT INTO bookmarks (id, account_id, subject_type, subject_id, note, is_public, created_at, updated_at)
         VALUES (?, ?, 'work', ?, '', 0, ?, ?)",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(account)
    .bind(&work_id)
    .bind(now)
    .bind(now)
    .execute(tdb.db().sqlite_pool().expect("sqlite"))
    .await
    .expect("bookmark");
}

async fn imported_signals(tdb: &TestDb, account: &str) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT origin FROM taste_signals WHERE account_id = ? AND source_key LIKE 'passport:%'",
    )
    .bind(account)
    .fetch_all(tdb.db().sqlite_pool().expect("sqlite"))
    .await
    .expect("signals")
}
