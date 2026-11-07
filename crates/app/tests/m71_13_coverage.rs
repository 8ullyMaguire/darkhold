//! M71-13: a page that enumerates a dimension says how much of the archive that dimension is.
//!
//! The failure this guards is quiet and it is everywhere on the web: a tag page saying "12 works" is read as "this tag is small",
//! when it often means "the archive is small" or "the tag is new". The reader's decision — keep browsing, or conclude the archive
//! has nothing for them — is made on that sentence, so the sentence has to carry the denominator.
//!
//! Asserts:
//! 1. the API's `coverage` names the view's works and the archive's published-public total, with a share that is the ratio of those
//!    two numbers and a sentence that states both;
//! 2. drafts, private works and deleted works are in neither number, so the share is a share of the archive a reader can reach;
//! 3. the document page prints the same sentence rather than an unqualified count (ADR 0006: the document is not a lesser surface);
//! 4. a tag whose works all went private says so, instead of printing a straight-faced `0.0%`.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::{Backend, Database, DatabaseConfig};
use serde_json::Value;
use std::path::{Path, PathBuf};
use tower::ServiceExt;

const T0: &str = "2026-01-01T00:00:00Z";
const ACCOUNT: &str = "01955000-0000-4000-8000-0000000e0001";
const PSEUD: &str = "01955000-0000-4000-8000-0000000e0002";
const SLOWBURN: &str = "01955000-0000-4000-8000-0000000e0010";
const NARUTO: &str = "01955000-0000-4000-8000-0000000e0011";
const PRIVATE_TAG: &str = "01955000-0000-4000-8000-0000000e0012";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-m7113-{tag}-{}-{:?}",
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
    config.site.name = "Archive".to_owned();
    config
}

struct Harness {
    dir: PathBuf,
    config: Config,
    db: Database,
}

impl Harness {
    async fn new(tag: &str) -> Self {
        let dir = scratch_dir(tag);
        let config = config_for(&dir);
        let database = Database::connect(&DatabaseConfig::new(format!(
            "sqlite://{}/m7113.sqlite?mode=rwc",
            dir.display()
        )))
        .await
        .expect("connect");
        database.migrate().await.expect("migrate");
        Self {
            dir,
            config,
            db: database,
        }
    }

    fn client(&self) -> axum::Router {
        server::build_router(AppState::new(self.config.clone(), self.db.clone()))
    }
}

async fn exec(db: &Database, statement: &str) {
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(statement)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await
                .unwrap_or_else(|error| panic!("{statement}\n{error}"));
        }
        Backend::Postgres => {
            sqlx::query(statement)
                .execute(db.postgres_pool().expect("postgres"))
                .await
                .unwrap_or_else(|error| panic!("{statement}\n{error}"));
        }
    }
}

async fn account(db: &Database) {
    exec(
        db,
        &format!(
            "INSERT INTO accounts (id, email, status, age_state, created_at, updated_at, permission_statement)
             VALUES ('{ACCOUNT}','cov@example.invalid','active','adult','{T0}','{T0}','minimal')"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at)
             VALUES ('{PSEUD}','{ACCOUNT}','covwriter','covwriter','{T0}','{T0}')"
        ),
    )
    .await;
}

async fn node(db: &Database, id: &str, kind: &str, canonical: &str) {
    exec(
        db,
        &format!(
            "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at, review_status, signal_count)
             VALUES ('{id}','{kind}','{canonical}','{}','{T0}','confirmed',0)",
            canonical.to_lowercase()
        ),
    )
    .await;
}

async fn work(
    db: &Database,
    index: usize,
    title: &str,
    lifecycle: &str,
    visibility: &str,
    tag: Option<&str>,
) {
    let id = format!("01956000-0000-4000-8000-0000000f00{index:02}");
    exec(
        db,
        &format!(
            "INSERT INTO works (id, owner_pseud_id, title, summary, lifecycle, visibility, completion,
                                created_at, updated_at, generated_content_posture)
             VALUES ('{id}','{PSEUD}','{title}','','{lifecycle}','{visibility}','complete','{T0}','{T0}','forbid')"
        ),
    )
    .await;
    if let Some(node_id) = tag {
        exec(
            db,
            &format!(
                "INSERT INTO work_tags (work_id, node_id, prominence, added_at)
                 VALUES ('{id}','{node_id}',1,'{T0}')"
            ),
        )
        .await;
    }
}

/// Three works carry the tag — one of them private, one a draft — and five published public works make the archive countable.
async fn seed(db: &Database) {
    account(db).await;
    node(db, SLOWBURN, "tag", "slow burn").await;
    node(db, NARUTO, "fandom", "Naruto").await;
    node(db, PRIVATE_TAG, "tag", "gone quiet").await;

    work(db, 1, "Public burn", "published", "public", Some(SLOWBURN)).await;
    work(db, 2, "Another burn", "published", "public", Some(SLOWBURN)).await;
    work(
        db,
        3,
        "Private burn",
        "published",
        "private",
        Some(SLOWBURN),
    )
    .await;
    work(db, 4, "Draft burn", "draft", "public", Some(SLOWBURN)).await;
    // Unrelated published works: they are the denominator.
    work(db, 5, "Plain one", "published", "public", None).await;
    work(db, 6, "Plain two", "published", "public", Some(NARUTO)).await;
    work(db, 7, "Plain three", "published", "public", None).await;
    // A tag whose only work went private: the page must not print 0.0%.
    work(db, 8, "Quiet", "published", "private", Some(PRIVATE_TAG)).await;
}

async fn json_of(app: axum::Router, uri: &str) -> (StatusCode, Value) {
    let response = app
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("body");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn html_of(app: axum::Router, uri: &str) -> (StatusCode, String) {
    let response: Response = app
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("body");
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

#[tokio::test]
async fn a_tag_page_states_the_share_of_the_archive_it_describes() {
    let harness = Harness::new("tag").await;
    seed(&harness.db).await;

    let (status, body) = json_of(harness.client(), "/api/v1/tags/slow%20burn").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let coverage = &body["coverage"];
    assert_eq!(coverage["dimension"], "tag", "{coverage}");
    assert_eq!(
        coverage["works"], 2,
        "the public works only: the private and the draft are not coverage: {coverage}"
    );
    assert_eq!(
        coverage["catalogue"], 5,
        "five published public works (1, 2, 5, 6, 7); the draft and the two private works are not: {coverage}"
    );
    let share = coverage["share"].as_f64().expect("share");
    assert!((share - 0.4).abs() < 1e-9, "2 of 5: {coverage}");
    let sentence = coverage["sentence"].as_str().expect("sentence");
    assert!(
        sentence.contains("2 of 5")
            && sentence.contains("tagged slow burn")
            && sentence.contains("40.0%"),
        "the sentence carries both numbers and the share: {sentence}"
    );

    // The fandom door publishes the same fact with its own wording.
    let (status, body) = json_of(harness.client(), "/api/v1/fandoms/Naruto").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["coverage"]["dimension"], "fandom");
    assert_eq!(body["coverage"]["works"], 1, "{body}");
    let sentence = body["coverage"]["sentence"].as_str().expect("sentence");
    assert!(
        sentence.contains("in Naruto"),
        "a fandom coverage sentence says the fandom, not 'this fandom': {sentence}"
    );

    // A tag whose only work is private answers with the fact that nothing public carries it.
    let (status, body) = json_of(harness.client(), "/api/v1/tags/gone%20quiet").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["coverage"]["works"], 0, "{body}");
    let sentence = body["coverage"]["sentence"].as_str().expect("sentence");
    assert!(
        sentence.contains("any more") && !sentence.contains("0.0%"),
        "it says the works went away rather than dressing zero as a percentage: {sentence}"
    );
}

#[tokio::test]
async fn the_document_tag_page_prints_the_same_coverage_sentence() {
    let harness = Harness::new("doc").await;
    seed(&harness.db).await;

    let (status, html) = html_of(harness.client(), "/tags/slow%20burn").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        html.contains("2 of 5 works") && html.contains("40.0%"),
        "the document carries the denominator, not a bare count: {}",
        &html[..html.len().min(1500)]
    );
    assert!(
        !html.contains(">2 works<"),
        "the unqualified count is gone: {}",
        &html[..html.len().min(1500)]
    );
    let _ = &harness.dir;
}
