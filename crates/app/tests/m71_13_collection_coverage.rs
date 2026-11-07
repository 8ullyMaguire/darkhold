//! M71-13, collection half: a collection states how much of its own list a visitor can open.
//!
//! A collection's member list is a promise — "these works belong together" — and some of those works are private, drafts or
//! deleted. `GET /api/v1/collections/{id}` used to publish `items` and whatever the collection row said, which reads as "these
//! are the works". The interesting ratio here is *inside* the collection, not against the archive: a hand-picked set has a
//! denominator nobody chose, so `collection_coverage` counts entries and readable entries and nothing else.
//!
//! Asserts:
//! 1. entries and readable are counted separately, and `hidden` is published rather than left to subtraction;
//! 2. a private work, a draft and a deleted work are each unreadable, and each of them is counted as *hidden* rather than dropped
//!    from the total — dropping it would make the collection look complete;
//! 3. the sentence distinguishes "all readable" from a gap and from an empty collection, because those are three different pages;
//! 4. the route carries the numbers and the sentence.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::{Backend, Database, DatabaseConfig};
use serde_json::Value;
use std::path::PathBuf;
use tower::ServiceExt;

const T0: &str = "2026-01-01T00:00:00Z";
const ACCOUNT: &str = "01955000-0000-4000-8000-000000130001";
const PSEUD: &str = "01955000-0000-4000-8000-000000130002";
const COLLECTION: &str = "01955000-0000-4000-8000-000000130010";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-m7113c-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

struct Harness {
    config: Config,
    db: Database,
    _dir: PathBuf,
}

impl Harness {
    async fn new(tag: &str) -> Self {
        let dir = scratch_dir(tag);
        let mut config = Config::development_defaults();
        config.storage.root = dir.clone();
        let database = Database::connect(&DatabaseConfig::new(format!(
            "sqlite://{}/m7113c.sqlite?mode=rwc",
            dir.display()
        )))
        .await
        .expect("connect");
        database.migrate().await.expect("migrate");
        Self {
            config,
            db: database,
            _dir: dir,
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

async fn seed(db: &Database) {
    exec(
        db,
        &format!(
            "INSERT INTO accounts (id, email, status, age_state, created_at, updated_at, permission_statement)
             VALUES ('{ACCOUNT}','col@example.invalid','active','adult','{T0}','{T0}','minimal')"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at)
             VALUES ('{PSEUD}','{ACCOUNT}','colwriter','colwriter','{T0}','{T0}')"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO collections (id, owner, name, description, item_policy, is_public, created_at)
             VALUES ('{COLLECTION}','{PSEUD}','Best of','','open',1,'{T0}')"
        ),
    )
    .await;

    let works = [
        ("01957000-0000-4000-8000-000000000101", "published", "public", None),
        ("01957000-0000-4000-8000-000000000102", "published", "public", None),
        ("01957000-0000-4000-8000-000000000103", "published", "private", None),
        ("01957000-0000-4000-8000-000000000104", "draft", "public", None),
        // Deleted but published and public: still unreadable, and the trap for a query that forgets `deleted_at`.
        ("01957000-0000-4000-8000-000000000105", "published", "public", Some(T0)),
    ];
    for (id, lifecycle, visibility, deleted_at) in works {
        let deleted = deleted_at.map(|at| format!("'{at}'")).unwrap_or_else(|| "NULL".to_owned());
        exec(
            db,
            &format!(
                "INSERT INTO works (id, owner_pseud_id, title, summary, lifecycle, visibility, completion,
                                    created_at, updated_at, deleted_at, generated_content_posture)
                 VALUES ('{id}','{PSEUD}','A work','','{lifecycle}','{visibility}','complete','{T0}','{T0}',{deleted},'forbid')"
            ),
        )
        .await;
        exec(
            db,
            &format!(
                "INSERT INTO collection_items (collection_id, work_id, added_by, added_at)
                 VALUES ('{COLLECTION}','{id}','{PSEUD}','{T0}')"
            ),
        )
        .await;
    }
}

async fn json_of(app: axum::Router, uri: &str) -> (StatusCode, Value) {
    let response = app
        .oneshot(Request::builder().uri(uri).body(Body::empty()).expect("request"))
        .await
        .expect("response");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("body");
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

#[tokio::test]
async fn a_collection_states_how_many_of_its_entries_a_visitor_can_open() {
    let harness = Harness::new("route").await;
    seed(&harness.db).await;

    let (status, body) = json_of(harness.client(), &format!("/api/v1/collections/{COLLECTION}")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let coverage = &body["coverage"];
    assert_eq!(coverage["entries"], 5, "the list is five long, private or not: {coverage}");
    assert_eq!(
        coverage["readable"], 2,
        "two are published and public and undeleted: {coverage}"
    );
    assert_eq!(
        coverage["hidden"], 3,
        "the private, the draft and the deleted one: {coverage}"
    );
    let sentence = coverage["sentence"].as_str().expect("sentence");
    assert!(
        sentence.contains("2 of 5") && sentence.contains("3 entries are private, unpublished or deleted"),
        "the gap is stated with its number and its reason: {sentence}"
    );
}

/// The three shapes of the sentence, because they are three different pages.
#[tokio::test]
async fn the_sentence_distinguishes_complete_partial_and_empty() {
    let harness = Harness::new("shapes").await;
    seed(&harness.db).await;

    // Delete the three unreadable entries' works from the list: what remains is two readable entries.
    exec(
        &harness.db,
        "DELETE FROM collection_items WHERE work_id IN (
            '01957000-0000-4000-8000-000000000103',
            '01957000-0000-4000-8000-000000000104',
            '01957000-0000-4000-8000-000000000105')",
    )
    .await;
    let complete = darkhold_db::events::collection_coverage(&harness.db, COLLECTION)
        .await
        .expect("coverage");
    assert_eq!((complete.entries, complete.readable), (2, 2));
    assert!(
        complete.sentence.contains("all of them readable"),
        "a complete collection says so: {}",
        complete.sentence
    );

    exec(
        &harness.db,
        &format!("DELETE FROM collection_items WHERE collection_id = '{COLLECTION}'"),
    )
    .await;
    let empty = darkhold_db::events::collection_coverage(&harness.db, COLLECTION)
        .await
        .expect("coverage");
    assert_eq!(empty.entries, 0);
    assert_eq!(
        empty.sentence, "This collection has no entries yet.",
        "an empty collection is not a gap in coverage"
    );
}
