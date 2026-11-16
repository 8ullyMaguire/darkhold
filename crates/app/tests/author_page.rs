//! ADR 0006 / M71-20 — an author's page is a document.
//!
//! Work-page bylines already link to `/people/{handle}`. Until this route existed that link landed on the SPA fallback:
//! a crawler saw a shell, and a reader with JavaScript off saw nothing. These tests pin the document that replaces it.
//!
//! 1. a handle lists that author's reachable works as work links, with no script element;
//! 2. a private work by the same author does not appear, because `search_works_ast_scoped` filters to published public
//!    works — the page does not add a weaker filter of its own;
//! 3. a handle with no works and no pseud is a 404 page, not an empty promise (§8.11).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server::{self, set_trust_proxy};
use darkhold_app::state::AppState;
use darkhold_db::DatabaseConfig;
use std::path::{Path, PathBuf};
use tower::ServiceExt;

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-authorpage-{tag}-{}-{:?}",
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

const ACCOUNT: &str = "01955000-0000-4000-8000-0000000002a1";
const PSEUD: &str = "01955000-0000-4000-8000-0000000002e1";
const PUBLIC_A: &str = "01957000-0000-4000-8000-0000000002a1";
const PUBLIC_B: &str = "01957000-0000-4000-8000-0000000002b1";
const PRIVATE_C: &str = "01957000-0000-4000-8000-0000000002c1";

struct Harness {
    _dir: PathBuf,
    tdb: test_support::TestDb,
    config: Config,
}

impl Harness {
    async fn new(tag: &str) -> Self {
        set_trust_proxy(false);
        let dir = scratch_dir(tag);
        let tdb = test_support::TestDb::connect_with_dir(tag, &dir).await;
        let config = config_for(&dir);
        Self {
            _dir: dir,
            tdb,
            config,
        }
    }

    fn client(&self) -> axum::Router {
        server::build_router(AppState::new(self.config.clone(), self.tdb.db().clone()))
    }

    async fn exec(&self, statement: &str) {
        let db = self.tdb.db();
        let executed = match db.backend() {
            darkhold_db::Backend::Sqlite => sqlx::query(statement)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await
                .map(|_| ()),
            darkhold_db::Backend::Postgres => sqlx::query(statement)
                .execute(db.postgres_pool().expect("postgres"))
                .await
                .map(|_| ()),
        };
        executed.unwrap_or_else(|error| panic!("{statement}\n{error}"));
    }

    async fn work(&self, id: &str, title: &str, visibility: &str) {
        self.exec(&format!(
            "INSERT INTO works (id, owner_pseud_id, title, language, rating, visibility, lifecycle,
                                created_at, updated_at, version)
             VALUES ('{id}','{PSEUD}',{title},'en','general','{visibility}','published',
                     '2026-01-01T00:00:00Z','2026-01-02T00:00:00Z',1)"
        ))
        .await;
    }

    async fn seed(&self) {
        self.exec(&format!(
            "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version)
             VALUES ('{ACCOUNT}','active','authorpage@example.invalid','adult',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ))
        .await;
        self.exec(&format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at,
                                 updated_at, version)
             VALUES ('{PSEUD}','{ACCOUNT}','hollowgast','Hollow Gast','listed',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ))
        .await;
        self.work(PUBLIC_A, "'The Long Way Round'", "public").await;
        self.work(PUBLIC_B, "'A Second Work'", "public").await;
        self.work(PRIVATE_C, "'A Private Work'", "private").await;
    }
}

async fn get(app: axum::Router, uri: &str) -> (StatusCode, String) {
    let response = app
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&body).into_owned())
}

#[tokio::test]
async fn an_author_page_lists_reachable_works_without_a_script() {
    let harness = Harness::new("list").await;
    harness.seed().await;

    let (status, body) = get(harness.client(), "/people/hollowgast").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!body.contains("<script"), "no JavaScript required: {body}");
    assert!(
        body.contains("hollowgast"),
        "the page names its author: {body}"
    );
    assert!(body.contains("The Long Way Round"), "{body}");
    assert!(body.contains("A Second Work"), "{body}");
    assert!(
        body.contains(&format!("/works/{PUBLIC_A}")),
        "each entry links the work page: {body}"
    );
    assert!(
        !body.contains("A Private Work"),
        "private work must not appear on a public author page: {body}"
    );
}

#[tokio::test]
async fn an_unknown_author_is_a_404_not_an_empty_page() {
    let harness = Harness::new("missing").await;
    harness.seed().await;

    let (status, body) = get(harness.client(), "/people/nobody-here").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(!body.contains("<script"), "{body}");
}
