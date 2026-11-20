//! M64-14 — the served feed actually uses the configured Ollama reranker.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server::{self, set_trust_proxy};
use darkhold_app::state::AppState;
use darkhold_db::DatabaseConfig;
use darkhold_domain::discovery::Candidate;
use darkhold_domain::embeddings::{EmbeddingError, EmbeddingInput, EmbeddingProvider};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tower::ServiceExt;

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-m64-14-feed-{tag}-{}-{:?}",
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
    for quota in [
        &mut config.rate_limits.auth,
        &mut config.rate_limits.write,
        &mut config.rate_limits.search,
        &mut config.rate_limits.default,
    ] {
        *quota = darkhold_app::limiter::Quota {
            burst: 1000,
            per_minute: 6000,
        };
    }
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
    fn cookie(&self, name: &str) -> Option<&str> {
        self.cookies
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
    fn capture(&mut self, response: &axum::response::Response) {
        for value in response.headers().get_all(header::SET_COOKIE) {
            let Ok(text) = value.to_str() else { continue };
            let Some((pair, _)) = text.split_once(';') else {
                continue;
            };
            if let Some((name, value)) = pair.split_once('=') {
                let name = name.trim().to_owned();
                let value = value.trim().to_owned();
                self.cookies.retain(|(k, _)| k != &name);
                if !value.is_empty() {
                    self.cookies.push((name, value));
                }
            }
        }
    }
    async fn request(
        &mut self,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder().method(method).uri(uri);
        if !self.cookies.is_empty() {
            builder = builder.header(
                header::COOKIE,
                self.cookies
                    .iter()
                    .map(|(n, v)| format!("{n}={v}"))
                    .collect::<Vec<_>>()
                    .join("; "),
            );
        }
        if !matches!(method, "GET" | "HEAD" | "OPTIONS") {
            if let Some(token) = self.cookie("darkhold_csrf").map(str::to_owned) {
                builder = builder.header("x-csrf-token", token);
            }
        }
        let request = match body {
            Some(v) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&v).expect("serialise")))
                .expect("request"),
            None => builder.body(Body::empty()).expect("request"),
        };
        let response = self.app.clone().oneshot(request).await.expect("response");
        let status = response.status();
        self.capture(&response);
        let bytes = axum::body::to_bytes(response.into_body(), 16 * 1024 * 1024)
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

struct Harness {
    dir: PathBuf,
    tdb: test_support::TestDb,
}

impl Harness {
    async fn new(tag: &str) -> Self {
        set_trust_proxy(false);
        let _ = darkhold_app::logging::init(&darkhold_app::config::LoggingConfig {
            filter: "error".to_owned(),
            format: darkhold_app::config::LogFormat::Pretty,
        });
        let dir = scratch_dir(tag);
        let tdb = test_support::TestDb::connect_with_dir(tag, &dir).await;
        Self { dir, tdb }
    }
    fn client(&self) -> Client {
        Client::new(server::build_router(AppState::new(
            config_for(&self.dir),
            self.tdb.db().clone(),
        )))
    }
    async fn cleanup(self) {
        self.tdb.cleanup().await;
        let _ = std::fs::remove_dir_all(self.dir);
    }
}

const PASSWORD: &str = "a-long-enough-passphrase";

async fn register(client: &mut Client, email: &str, handle: &str) {
    let (status, body) = client
        .post(
            "/api/v1/auth/register",
            json!({
                "email": email,
                "password": PASSWORD,
                "handle": handle,
                "display_name": handle,
                "age_band": "adult"
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "register {handle}: {body}");
}

async fn published_work(
    harness: &Harness,
    email: &str,
    handle: &str,
    title: &str,
    summary: &str,
) -> String {
    let mut author = harness.client();
    register(&mut author, email, handle).await;
    let (status, body) = author
        .post("/api/v1/works", json!({ "title": title }))
        .await;
    assert_eq!(status, StatusCode::CREATED, "create work: {body}");
    let work_id = body["id"].as_str().expect("id").to_owned();
    let version = body["version"].as_i64().expect("version");
    let (status, body) = author
        .request(
            "PATCH",
            &format!("/api/v1/works/{work_id}"),
            Some(json!({ "expected_version": version, "summary": summary })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "summary: {body}");
    let version = body["version"].as_i64().expect("summary version");
    let (status, body) = author
        .post(
            &format!("/api/v1/works/{work_id}/chapters"),
            json!({ "title": "One" }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "unit: {body}");
    let unit = body["id"].as_str().expect("unit").to_owned();
    let unit_version = body["version"].as_i64().expect("unit version");
    let doc = json!({ "type": "doc", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "A unit with enough words to have a middle." }] }] });
    let (status, _) = author
        .request(
            "PATCH",
            &format!("/api/v1/chapters/{unit}"),
            Some(json!({ "expected_version": unit_version, "document": doc })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "save unit");
    let (status, _) = author
        .post(
            &format!("/api/v1/works/{work_id}/publish"),
            json!({ "expected_version": version, "idempotency_key": format!("m64-14-{work_id}") }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "publish");
    work_id
}

async fn account_id(tdb: &test_support::TestDb, email: &str) -> String {
    let db = tdb.db();
    match db.backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query_scalar("SELECT id FROM accounts WHERE email = ?")
                .bind(email)
                .fetch_one(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("account id")
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query_scalar("SELECT id::text FROM accounts WHERE email = $1")
                .bind(email)
                .fetch_one(db.postgres_pool().expect("postgres"))
                .await
                .expect("account id")
        }
    }
}

async fn set_taste_profile(tdb: &test_support::TestDb, account: &str, signals: &Value) {
    let db = tdb.db();
    let signals = signals.to_string();
    match db.backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO taste_profiles (account, signals, computed_at) VALUES (?, ?, '2026-11-20T00:00:00Z')",
            )
            .bind(account)
            .bind(signals)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("taste profile");
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(
                "INSERT INTO taste_profiles (account, signals, computed_at) VALUES ($1, $2, '2026-11-20T00:00:00Z')",
            )
            .bind(account)
            .bind(signals)
            .execute(db.postgres_pool().expect("postgres"))
            .await
            .expect("taste profile");
        }
    }
}

struct DragonProvider;

impl EmbeddingProvider for DragonProvider {
    fn available(&self) -> bool {
        true
    }

    fn embed<'a>(
        &'a self,
        input: &'a EmbeddingInput,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<f32>, EmbeddingError>> + Send + 'a>> {
        Box::pin(async move {
            if input.text.to_ascii_lowercase().contains("dragon") {
                Ok(vec![1.0, 0.0])
            } else {
                Ok(vec![0.0, 1.0])
            }
        })
    }
}

async fn mock_ollama() -> (String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock ollama");
    let address = listener.local_addr().expect("address");
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let server = tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let mut buffer = [0u8; 8192];
            let _ = socket.read(&mut buffer).await;
            counter.fetch_add(1, Ordering::SeqCst);
            let body = r#"{"data":[{"embedding":[0.5,0.5]}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
        }
    });
    (
        format!("http://{address}/v1/embeddings"),
        calls,
        server,
    )
}

#[tokio::test]
async fn embedding_reranker_moves_the_matching_work_first() {
    let harness = Harness::new("rerank").await;
    let garden = published_work(
        &harness,
        "garden@example.com",
        "Gardener",
        "Garden",
        "A quiet garden with roses.",
    )
    .await;
    let dragon = published_work(
        &harness,
        "dragon@example.com",
        "Rider",
        "Dragon",
        "A dragon rider crosses the mountains.",
    )
    .await;
    let mut reader = harness.client();
    register(&mut reader, "reader@example.com", "Reader").await;
    let account = account_id(&harness.tdb, "reader@example.com").await;
    set_taste_profile(&harness.tdb, &account, &json!({ "fandom": ["dragon"] })).await;

    let ranked = vec![
        Candidate {
            work_id: garden.parse().expect("garden id"),
            score: 10,
            reason: "alpha".to_owned(),
            taste_signal: 0.0,
            diversity_class: 0.0,
        },
        Candidate {
            work_id: dragon.parse().expect("dragon id"),
            score: 9,
            reason: "alpha".to_owned(),
            taste_signal: 0.0,
            diversity_class: 0.0,
        },
    ];
    let reranked = darkhold_app::discovery_embedding::rerank_with_provider(
        harness.tdb.db(),
        &account,
        ranked,
        &DragonProvider,
    )
    .await;
    assert_eq!(reranked[0].work_id.to_string(), dragon, "{reranked:?}");
    assert_eq!(reranked[1].work_id.to_string(), garden, "{reranked:?}");
    harness.cleanup().await;
}

#[tokio::test]
async fn served_feed_calls_the_configured_provider() {
    let (endpoint, calls, server) = mock_ollama().await;
    std::env::set_var("DARKHOLD_OLLAMA_URL", &endpoint);
    std::env::set_var("DARKHOLD_OLLAMA_MODEL", "test-model");

    let harness = Harness::new("served").await;
    let _ = published_work(
        &harness,
        "author@example.com",
        "Author",
        "Dragon",
        "A dragon rider crosses the mountains.",
    )
    .await;
    let _ = published_work(
        &harness,
        "garden@example.com",
        "Gardener",
        "Garden",
        "A quiet garden with roses.",
    )
    .await;
    let mut reader = harness.client();
    register(&mut reader, "reader@example.com", "Reader").await;
    let account = account_id(&harness.tdb, "reader@example.com").await;
    set_taste_profile(&harness.tdb, &account, &json!({ "fandom": ["dragon"] })).await;

    let (status, body) = reader.get("/api/v1/discovery").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        calls.load(Ordering::SeqCst) > 0,
        "the served feed never called the configured provider: {body}"
    );

    std::env::remove_var("DARKHOLD_OLLAMA_URL");
    std::env::remove_var("DARKHOLD_OLLAMA_MODEL");
    server.abort();
    harness.cleanup().await;
}
