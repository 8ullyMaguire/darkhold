//! M63-05b — the built rendition has bytes a client can fetch.
//!
//! Until this route existed a derivative was a row describing a file: `output_checksum`, `output_bytes`,
//! `output_mime_type`, and no door. `dh rec --link epub` refused with "needs a download route this instance does not
//! have", which was true and unfixable from the CLI.
//!
//! What the route can get wrong, and what each test below pins:
//!
//! 1. **A queued derivative must not answer an empty 200.** A job that has not finished has no bytes; the answer is
//!    "no built derivative", and the metadata route carries the state for a caller that wants to know why.
//! 2. **The bytes served are the bytes stored**, with the recorded content type and an attachment disposition — a
//!    rendition that arrives as `text/html` or with no filename is a broken download even when the status is 200.

use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::derivative::{
    create_derivative, mark_derivative_built, mark_derivative_stale, NewDerivative,
};
use darkhold_domain::derivative::DerivativeKind;
use darkhold_db::storage::BlobStore;
use darkhold_db::{Database, DatabaseConfig};
use test_support::TestDb;
use tower::ServiceExt;

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-m63-05b-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn config_for(dir: &Path) -> Config {
    let mut config = Config::development_defaults();
    config.storage.root = dir.to_path_buf();
    config.database = match std::env::var("DARKHOLD_TEST_PG_URL") {
        Ok(url) if !url.is_empty() => DatabaseConfig::new(url),
        _ => DatabaseConfig::new(format!("sqlite://{}/darkhold.sqlite?mode=rwc", dir.display())),
    };
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
        let config = config_for(&dir);
        let tdb = TestDb::connect_with_dir(tag, &dir).await;
        Self { _dir: dir, tdb, config }
    }

    fn db(&self) -> &Database {
        self.tdb.db()
    }

    fn router(&self) -> axum::Router {
        server::build_router(AppState::new(self.config.clone(), self.tdb.db().clone()))
    }

    /// A published, public work with an owner — `reading_for_work` decides on exactly those fields.
    async fn work(&self) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let account = uuid::Uuid::new_v4().to_string();
        let owner = uuid::Uuid::new_v4().to_string();
        let stmts = [
            format!(
                "INSERT INTO accounts (id, email, created_at, updated_at) \
                 VALUES ('{account}', 'm6305b-{account}@example.test', '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00')"
            ),
            format!(
                "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at) \
                 VALUES ('{owner}', '{account}', 'm{owner}', 'M', '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00')"
            ),
            format!(
                "INSERT INTO works (id, owner_pseud_id, title, visibility, lifecycle, created_at, updated_at) \
                 VALUES ('{id}', '{owner}', 'A Work', 'public', 'published', '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00')"
            ),
        ];
        for statement in stmts {
            self.exec(&statement).await;
        }
        id
    }

    async fn exec(&self, statement: &str) {
        match self.db().backend() {
            darkhold_db::Backend::Sqlite => {
                sqlx::query(statement)
                    .execute(self.db().sqlite_pool().expect("sqlite"))
                    .await
                    .expect("seed statement");
            }
            darkhold_db::Backend::Postgres => {
                sqlx::query(statement)
                    .execute(self.db().postgres_pool().expect("postgres"))
                    .await
                    .expect("seed statement");
            }
        }
    }

    /// A derivative in `queued` — created through the repository, the way the route does.
    async fn queued(&self, work: &str) -> String {
        create_derivative(
            self.db(),
            NewDerivative {
                work_id: work,
                edition_kind: "draft",
                derivative_kind: DerivativeKind::Epub,
                parent_checksum: "parent0000000000000000000000000000000000000000000000000000000000000",
            },
        )
        .await
        .expect("create_derivative")
    }
}

async fn get(app: axum::Router, uri: &str) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec();
    (status, headers, bytes)
}

#[tokio::test]
async fn a_built_rendition_is_served_as_the_file_it_is() {
    let harness = Harness::new("served").await;
    let work = harness.work().await;
    let id = harness.queued(&work).await;

    let payload = b"PK\x03\x04 an epub, in spirit".to_vec();
    let store = BlobStore::new(harness.config.storage.root.clone());
    let (checksum, _key) = store
        .put(harness.db(), &payload, "application/epub+zip")
        .await
        .expect("store the rendition");
    mark_derivative_built(
        harness.db(),
        &id,
        &checksum,
        payload.len() as i64,
        "application/epub+zip",
    )
    .await
    .expect("mark built");

    let (status, headers, body) = get(harness.router(), &format!("/api/v1/derivatives/{id}/download")).await;

    assert_eq!(status, StatusCode::OK, "a built rendition downloads");
    assert_eq!(
        headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()),
        Some("application/epub+zip"),
        "the recorded content type is what a client saves by"
    );
    let disposition = headers
        .get(header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    assert!(
        disposition.starts_with("attachment") && disposition.contains(".epub"),
        "a download needs a filename a reader can find: {disposition}"
    );
    assert_eq!(body, payload, "the bytes served are the bytes stored");
}

#[tokio::test]
async fn a_queued_rendition_has_no_file_to_serve() {
    let harness = Harness::new("queued").await;
    let work = harness.work().await;
    let id = harness.queued(&work).await;

    let (status, _headers, body) = get(harness.router(), &format!("/api/v1/derivatives/{id}/download")).await;

    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a job that has not finished has no bytes: {}",
        String::from_utf8_lossy(&body)
    );
}

/// A rendition that was built and then went stale keeps its checksum, so the presence of an output is not the same
/// question as "is this the rendition a reader should get". This is the assertion the state filter exists for: without
/// it a stale file is served with a 200 and nobody can tell it from a current one.
#[tokio::test]
async fn a_stale_rendition_is_not_served_even_though_its_bytes_are_still_there() {
    let harness = Harness::new("stale").await;
    let work = harness.work().await;
    let id = harness.queued(&work).await;

    let payload = b"old epub".to_vec();
    let store = BlobStore::new(harness.config.storage.root.clone());
    let (checksum, _key) = store
        .put(harness.db(), &payload, "application/epub+zip")
        .await
        .expect("store the rendition");
    mark_derivative_built(
        harness.db(),
        &id,
        &checksum,
        payload.len() as i64,
        "application/epub+zip",
    )
    .await
    .expect("mark built");
    mark_derivative_stale(harness.db(), &id)
        .await
        .expect("mark stale");

    let (status, _headers, body) = get(harness.router(), &format!("/api/v1/derivatives/{id}/download")).await;

    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a stale rendition has a checksum and no claim to be served: {}",
        String::from_utf8_lossy(&body)
    );
}
