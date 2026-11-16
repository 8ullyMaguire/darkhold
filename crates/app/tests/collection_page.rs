//! ADR 0006 / M71-20 — a collection's page is a document.
//!
//! Collections are linkable archive objects: a reader arrives from a shelf, a recommendation or a shared URL. Until now
//! `/collections/{id}` fell through to the SPA shell, so none of those arrivals carried content. These tests pin the
//! document: the collection's own facts, its reachable works as work links, and no script.
//!
//! The private collection is a 404, not a teaser (§8.9), and a work the reader may not see is absent from the list even
//! when `collection_items` names it. The page uses the events collection model, not `media_collections`.

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
        "darkhold-collectionpage-{tag}-{}-{:?}",
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

const ACCOUNT: &str = "01955000-0000-4000-8000-0000000003a1";
const PSEUD: &str = "01955000-0000-4000-8000-0000000003e1";
const PUBLIC_A: &str = "01957000-0000-4000-8000-0000000003a1";
const PUBLIC_B: &str = "01957000-0000-4000-8000-0000000003b1";
const PRIVATE_C: &str = "01957000-0000-4000-8000-0000000003c1";

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

    async fn seed(&self) -> (String, String) {
        self.exec(&format!(
            "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version)
             VALUES ('{ACCOUNT}','active','collectionpage@example.invalid','adult',
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

        let public = darkhold_db::events::create_collection(
            self.tdb.db(),
            "A Reading Shelf",
            Some("Works worth returning to"),
            ACCOUNT,
            "any",
            true,
        )
        .await
        .expect("create public collection");
        for id in [PUBLIC_A, PUBLIC_B, PRIVATE_C] {
            darkhold_db::events::add_collection_item(self.tdb.db(), &public, id, ACCOUNT, None)
                .await
                .expect("add item");
        }
        let private = darkhold_db::events::create_collection(
            self.tdb.db(),
            "Private Shelf",
            None,
            ACCOUNT,
            "any",
            false,
        )
        .await
        .expect("create private collection");
        (public, private)
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
async fn a_public_collection_is_a_document_with_reachable_work_links() {
    let harness = Harness::new("list").await;
    let (public, _) = harness.seed().await;

    let (status, body) = get(harness.client(), &format!("/collections/{public}")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!body.contains("<script"), "no JavaScript required: {body}");
    assert!(
        body.contains("A Reading Shelf"),
        "names the collection: {body}"
    );
    assert!(
        body.contains("Works worth returning to"),
        "keeps the description: {body}"
    );
    assert!(body.contains("The Long Way Round"), "{body}");
    assert!(body.contains("A Second Work"), "{body}");
    assert!(
        body.contains(&format!("/works/{PUBLIC_A}")),
        "each entry links the work page: {body}"
    );
    assert!(
        !body.contains("A Private Work"),
        "a private member must not appear on a public shelf: {body}"
    );
}

#[tokio::test]
async fn a_private_or_unknown_collection_is_a_404() {
    let harness = Harness::new("hidden").await;
    let (_, private) = harness.seed().await;
    let app = harness.client();

    let (status, body) = get(app.clone(), &format!("/collections/{private}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(!body.contains("<script"), "{body}");
    let (status, body) = get(app, "/collections/01957000-0000-4000-8000-000000000000").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(!body.contains("<script"), "{body}");
}
