//! M70-02 — whole-work reading over HTTP: spans, totals, and a resolved place.
//!
//! Asserted here:
//!
//! * **Order and cumulative starts** come from `order_key`, not insertion order — a chapter inserted between two
//!   others is the case the spacing exists for.
//! * **A chapter with no saved revision is zero words**, not an error: it is a draft state a reader can meet.
//! * **`?at=` is resolved to a canonical place**, including the boundary where `(0, 100)` and `(1, 0)` are the same
//!   place and the earlier chapter wins.
//! * **An unknown chapter is 422**, so a client cannot be silently moved somewhere it never was.

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server::{self, set_trust_proxy};
use darkhold_app::state::AppState;
use darkhold_db::DatabaseConfig;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tower::ServiceExt;

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-whole-{tag}-{}-{:?}",
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

const WORK: &str = "01957000-0000-4000-8000-b7266946880a";
const CH1: &str = "01956000-0000-4000-8000-000000000c01";
// Revision ids are valid UUIDs because PostgreSQL types the column `uuid`; a fixture id only SQLite can hold ("r1")
// passes the whole suite locally and fails on the deployed engine with `invalid input syntax for type uuid`.
const RV1: &str = "0195a000-0000-4000-8000-0000000000b1";
const RV3: &str = "0195a000-0000-4000-8000-0000000000b3";
const CH2: &str = "01956000-0000-4000-8000-000000000c02";
const CH3: &str = "01956000-0000-4000-8000-000000000c03";

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

    fn app(&self) -> axum::Router {
        server::build_router(AppState::new(self.config.clone(), self.tdb.db().clone()))
    }

    /// Run a fixture statement on whichever engine the suite is running against. The statements are literals and `?`
    /// binds; `exec_one` rewrites the placeholders for PostgreSQL, and an INSERT of literal values needs no cast because
    /// assignment coerces. Reaching for `sqlite_pool()` here is what made this whole file invisible to the deployed engine.
    async fn exec(&self, statement: &str) {
        test_support::exec_one(self.tdb.db(), statement).await;
    }

    /// Three chapters at order keys 10/20/30, the middle one empty (no saved revision), and a fourth chapter that
    /// is soft-deleted so it must not appear at all.
    async fn seed(&self) {
        self.exec(
            "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version) \
             VALUES ('01955000-0000-4000-8000-0000000000a1','active','reader@whole.invalid','adult', \
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)",
        )
        .await;
        self.exec(
            "INSERT INTO pseuds (id, account_id, handle, display_name, bio, discoverability, created_at, \
             updated_at, version) \
             VALUES ('01955000-0000-4000-8000-89e97c100c54','01955000-0000-4000-8000-0000000000a1', \
                     'hollowgast','hollowgast',NULL,'listed','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)",
        )
        .await;
        self.exec(&format!(
            "INSERT INTO works (id, owner_pseud_id, title, summary, language, rating, visibility, lifecycle, \
             completion, created_at, updated_at, version) \
             VALUES ('{WORK}','01955000-0000-4000-8000-89e97c100c54','The Long Way Round','','en','general', \
                     'public','published','complete','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ))
        .await;
        for (id, key, title) in [(CH1, 10, "One"), (CH2, 20, "Two"), (CH3, 30, "Three")] {
            self.exec(&format!(
                "INSERT INTO units (id, work_id, order_key, title, current_revision_id, created_at, \
                 updated_at, version) VALUES ('{id}','{WORK}',{key},'{title}',NULL, \
                 '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
            ))
            .await;
        }
        // The first chapter has 100 words, the third 40; the second stays without a revision.
        for (chapter, revision, words) in [(CH1, RV1, 100), (CH3, RV3, 40)] {
            self.exec(&format!(
                "INSERT INTO revisions (id, unit_id, revision_number, document_json, sanitized_html, plain_text, \
                 word_count, created_by_pseud_id, created_at) \
                 VALUES ('{revision}','{chapter}',1,'{{}}','<p>x</p>','x',{words}, \
                         '01955000-0000-4000-8000-89e97c100c54','2026-01-01T00:00:00Z')"
            ))
            .await;
            self.exec(&format!(
                "UPDATE units SET current_revision_id = '{revision}' WHERE id = '{chapter}'"
            ))
            .await;
        }
        // A fourth chapter, deleted: absent from every answer.
        self.exec(&format!(
            "INSERT INTO units (id, work_id, order_key, title, current_revision_id, created_at, updated_at, \
             version, deleted_at) VALUES ('01956000-0000-4000-8000-000000000c04','{WORK}',25,'Gone',NULL, \
             '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'2026-02-01T00:00:00Z')"
        ))
        .await;
    }
}

async fn get(app: &axum::Router, uri: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json = serde_json::from_slice(&body)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&body).into()));
    (status, json)
}

#[tokio::test]
async fn spans_are_in_reading_order_and_a_draft_chapter_is_zero_words() {
    let h = Harness::new("spans").await;
    h.seed().await;
    let (status, body) = get(&h.app(), &format!("/api/v1/works/{WORK}/whole")).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    assert_eq!(body["total_words"], 140, "100 + 0 + 40");
    let chapters = body["chapters"].as_array().expect("chapters");
    assert_eq!(chapters.len(), 3, "the deleted chapter is not a chapter");
    assert_eq!(chapters[0]["chapter_id"], CH1);
    assert_eq!(
        chapters[1]["chapter_id"], CH2,
        "order_key 20 sits between 10 and 30"
    );
    assert_eq!(chapters[2]["chapter_id"], CH3);
    assert_eq!(
        chapters[1]["words"], 0,
        "a chapter with no saved revision is a real state"
    );
    assert_eq!(
        chapters[2]["start"], 100,
        "starts are cumulative, and the empty chapter adds nothing"
    );
    assert_eq!(
        body["position"],
        Value::Null,
        "no `at` means no place asked for"
    );
}

#[tokio::test]
async fn a_position_is_resolved_to_a_canonical_place_or_refused() {
    let h = Harness::new("position").await;
    h.seed().await;
    let app = h.app();

    // Chapter three has 40 words and starts at 100, so ten words in is 110 into the work. (Chapter two is the
    // draft with no revision, which is why it cannot carry a position — its whole length is zero.)
    let (status, body) = get(&app, &format!("/api/v1/works/{WORK}/whole?at={CH3}:10")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["position"]["chapter"], 2);
    assert_eq!(body["position"]["chapter_id"], CH3);
    assert_eq!(body["position"]["words_before"], 100);
    assert_eq!(body["position"]["words_into_work"], 110);

    // The boundary: the start of chapter two is the same place as the end of chapter one, and the earlier chapter
    // wins so a client that stores the answer asks about the same place next time.
    let (status, body) = get(&app, &format!("/api/v1/works/{WORK}/whole?at={CH3}:0")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["position"]["chapter"], 0,
        "canonical form is the end of the earlier chapter"
    );
    assert_eq!(body["position"]["chapter_id"], CH1);
    assert_eq!(body["position"]["offset"], 100);

    // An offset past the end of its chapter is not silently clamped.
    let (status, body) = get(&app, &format!("/api/v1/works/{WORK}/whole?at={CH1}:101")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    // A chapter that is not in this work is refused, and the message says what was expected.
    let (status, body) = get(
        &app,
        &format!("/api/v1/works/{WORK}/whole?at=not-a-chapter:1"),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("not in this work"),
        "{body}"
    );

    // A malformed `at` names the shape rather than failing to parse.
    let (status, body) = get(&app, &format!("/api/v1/works/{WORK}/whole?at={CH1}")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("<chapter id>:<offset>"),
        "{body}"
    );
}
