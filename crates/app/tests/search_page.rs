//! ADR 0006 / M71-20 — search results as a document.
//!
//! What this file proves:
//! 1. an anonymous reader gets search hits as HTML with no script, real work links and a no-JS form;
//! 2. the listing uses the same published-public filter as the JSON search, so a private work is absent;
//! 3. a signed-in reader keeps the SPA, because save-search and add-to-shelf are application actions.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server::{self, set_trust_proxy};
use darkhold_app::state::AppState;
use darkhold_db::DatabaseConfig;
use std::path::{Path, PathBuf};
use tower::ServiceExt;

const ACCOUNT: &str = "01955000-0000-4000-8000-0000000001a1";
const PSEUD: &str = "01955000-0000-4000-8000-0000000001e1";
const PUBLIC_A: &str = "01957000-0000-4000-8000-0000000001a1";
const PUBLIC_B: &str = "01957000-0000-4000-8000-0000000001b1";
const PRIVATE_C: &str = "01957000-0000-4000-8000-0000000001c1";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-searchpage-{tag}-{}-{:?}",
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
        let result = match db.backend() {
            darkhold_db::Backend::Sqlite => sqlx::query(statement)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await
                .map(|_| ()),
            darkhold_db::Backend::Postgres => sqlx::query(statement)
                .execute(db.postgres_pool().expect("postgres"))
                .await
                .map(|_| ()),
        };
        result.unwrap_or_else(|error| panic!("{statement}\n{error}"));
    }

    async fn seed(&self) {
        self.exec(&format!(
            "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version)
             VALUES ('{ACCOUNT}','active','searchpage@example.invalid','adult',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ))
        .await;
        self.exec(&format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at,
                                 updated_at, version)
             VALUES ('{PSEUD}','{ACCOUNT}','harbourkeeper','Harbour Keeper','listed',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ))
        .await;
        for (id, title, visibility) in [
            (PUBLIC_A, "Harbour Lights", "public"),
            (PUBLIC_B, "A Second Harbour Work", "public"),
            (PRIVATE_C, "Harbour Secret", "private"),
        ] {
            self.exec(&format!(
                "INSERT INTO works (id, owner_pseud_id, title, summary, language, rating, visibility,
                                    lifecycle, created_at, updated_at, version)
                 VALUES ('{id}','{PSEUD}','{title}','A story about the harbour.','en','general',
                         '{visibility}','published','2026-01-01T00:00:00Z',
                         '2026-01-01T00:00:00Z',1)"
            ))
            .await;
        }
    }
}

async fn get(app: axum::Router, uri: &str, cookie: Option<&str>) -> (StatusCode, String) {
    let mut builder = Request::builder().uri(uri);
    if let Some(cookie) = cookie {
        builder = builder.header("cookie", format!("darkhold_session={cookie}"));
    }
    let response = app
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&body).into_owned())
}

#[tokio::test]
async fn an_anonymous_search_is_a_document_with_real_links() {
    let harness = Harness::new("anonymous").await;
    harness.seed().await;

    let (status, body) = get(harness.client(), "/search?q=Harbour", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!body.contains("<script"), "no JavaScript required: {body}");
    assert_eq!(
        body.matches("aria-current=\"page\"").count(),
        1,
        "one active Darkhold destination: {body}"
    );
    assert!(
        body.contains("aria-label=\"Darkhold\"") && body.contains(">Search</a>"),
        "the search document carries the Darkhold nav: {body}"
    );
    assert!(body.contains("Harbour Lights"), "{body}");
    assert!(body.contains("A Second Harbour Work"), "{body}");
    assert!(
        body.contains(&format!("/works/{PUBLIC_A}")),
        "each hit links the work document: {body}"
    );
    assert!(
        body.contains("harbourkeeper"),
        "byline is on the page: {body}"
    );
    assert!(
        body.contains("live"),
        "status is named, not only styled: {body}"
    );
    assert!(
        !body.contains("Harbour Secret"),
        "private work is absent: {body}"
    );
    assert!(
        body.contains("source unmeasured"),
        "evidence says when a source clock is absent: {body}"
    );
    test_support::assert_no_empty_section(&body, "search document");
}

#[tokio::test]
async fn the_search_document_has_a_form_that_needs_no_script() {
    let harness = Harness::new("form").await;
    harness.seed().await;

    let (status, body) = get(harness.client(), "/search?q=Harbour", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.contains("action=\"/search\"") && body.contains("method=\"get\""),
        "a real GET form, not a button with a handler: {body}"
    );
    assert!(
        body.contains("name=\"q\""),
        "the query field has a name: {body}"
    );
}

#[tokio::test]
async fn a_signed_in_reader_keeps_the_search_application() {
    let harness = Harness::new("member").await;
    harness.seed().await;
    const TOKEN: &str = "search-page-test-session-token";
    let hash = darkhold_app::crypto::hash_token(TOKEN);
    harness
        .exec(&format!(
            "INSERT INTO sessions (id, account_id, token_hash, csrf_token_hash, user_agent,
                                   active_pseud_id, created_at, last_seen_at, expires_at, acr, roles)
             VALUES ('01956000-0000-4000-8000-0000000001a1','{ACCOUNT}','{hash}','{hash}',
                     'test','{PSEUD}','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',
                     '2099-01-01T00:00:00Z','pwd','[]')"
        ))
        .await;

    let (status, body) = get(harness.client(), "/search?q=Harbour", Some(TOKEN)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("<script"), "the member keeps the SPA: {body}");
}

#[tokio::test]
async fn constraints_are_named_get_fields_in_a_disclosure() {
    let harness = Harness::new("constraints").await;
    harness.seed().await;

    let (status, body) = get(harness.client(), "/search?q=Harbour", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("<details"), "constraints must be visible without script: {body}");
    assert!(body.contains(">Constraints</summary>"), "{body}");
    for name in [
        "fandom", "relationship", "completion", "length", "warning", "source",
        "language", "availability", "reading_status", "sort",
    ] {
        assert!(body.contains(&format!("name=\"{name}\"")), "missing GET field {name}: {body}");
    }
}

#[tokio::test]
async fn an_unknown_sort_is_named_instead_of_replaced() {
    let harness = Harness::new("bad-sort").await;
    harness.seed().await;

    let (status, body) = get(harness.client(), "/search?q=Harbour&sort=nope", None).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body.contains("Unsupported sort: nope"), "{body}");
}

#[tokio::test]
async fn zero_results_name_the_narrowest_constraint_to_remove() {
    let harness = Harness::new("zero").await;
    harness.seed().await;

    let (status, body) = get(harness.client(), "/search?q=Harbour&source=nowhere", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.contains("No works match these constraints. Remove the last constraint"),
        "{body}"
    );
    assert!(body.contains("source=nowhere"), "the narrowest constraint must be named: {body}");
}
