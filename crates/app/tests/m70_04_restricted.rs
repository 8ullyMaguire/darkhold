//! M70-04 — a restricted work: the record for everyone, the body for a session.
//!
//! The decision (2026-11-05): *listed but locked*. A stranger may find the work and read its metadata; the text and
//! the comments need a session, and the refusal is indistinguishable from a work that does not exist.

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
        "darkhold-restricted-{tag}-{}-{:?}",
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

const WORK: &str = "01957000-0000-4000-8000-0000000000d0";
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
             VALUES ('01955000-0000-4000-8000-0000000000e1','01955000-0000-4000-8000-0000000000a1', \
                     'hollowgast','hollowgast',NULL,'listed','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)",
        )
        .await;
        self.exec(&format!(
            "INSERT INTO works (id, owner_pseud_id, title, summary, language, rating, visibility, lifecycle, \
             completion, created_at, updated_at, version) \
             VALUES ('{WORK}','01955000-0000-4000-8000-0000000000e1','The Long Way Round','','en','general', \
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
                         '01955000-0000-4000-8000-0000000000e1','2026-01-01T00:00:00Z')"
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

const RESTRICTED: &str = "01957000-0000-4000-8000-0000000000d1";
const UNIT: &str = "01956000-0000-4000-8000-0000000000d2";
const PRIVATE: &str = "01957000-0000-4000-8000-0000000000d4";
const PRIVATE_UNIT: &str = "01956000-0000-4000-8000-0000000000d5";

impl Harness {
    /// A published, restricted work with one readable unit.
    async fn seed_restricted(&self) {
        self.exec(&format!(
            "INSERT INTO works (id, owner_pseud_id, title, summary, visibility, lifecycle, completion, \
                                published_at, created_at, updated_at) \
             VALUES ('{RESTRICTED}', '01955000-0000-4000-8000-0000000000e1', 'Held close', 'A summary a \
                     stranger may read.', 'restricted', 'published', 'complete', \
                     '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        ))
        .await;
        self.exec(&format!(
            "INSERT INTO units (id, work_id, order_key, title, current_revision_id, created_at, updated_at, version) \
             VALUES ('{UNIT}', '{RESTRICTED}', 10, 'One', NULL, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1)"
        ))
        .await;
        self.exec(&format!(
            "INSERT INTO revisions (id, unit_id, revision_number, document_json, sanitized_html, plain_text, \
                                   word_count, created_by_pseud_id, created_at) \
             VALUES ('01956000-0000-4000-8000-0000000000d3', '{UNIT}', 1, '{{}}', '<p>The body nobody signed in \
                     for.</p>', 'The body nobody signed in for.', 6, \
                     '01955000-0000-4000-8000-0000000000e1', '2026-01-01T00:00:00Z')"
        ))
        .await;
        self.exec(&format!(
            "UPDATE units SET current_revision_id = '01956000-0000-4000-8000-0000000000d3' WHERE id = '{UNIT}'"
        ))
        .await;
        // The control: a private work, whose existence is itself not public.
        self.exec(&format!(
            "INSERT INTO works (id, owner_pseud_id, title, summary, visibility, lifecycle, completion, \
                                created_at, updated_at) \
             VALUES ('{PRIVATE}', '01955000-0000-4000-8000-0000000000e1', 'Behind the wall', 'not for you', \
                     'private', 'published', 'complete', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        ))
        .await;
        self.exec(&format!(
            "INSERT INTO units (id, work_id, order_key, title, current_revision_id, created_at, updated_at, version) \
             VALUES ('{PRIVATE_UNIT}', '{PRIVATE}', 10, 'One', NULL, '2026-01-01T00:00:00Z', \
                     '2026-01-01T00:00:00Z', 1)"
        ))
        .await;
    }
}

#[tokio::test]
async fn a_stranger_gets_the_record_and_never_the_text() {
    let h = Harness::new("restricted").await;
    h.seed().await;
    h.seed_restricted().await;
    let app = server::build_router(AppState::new(h.config.clone(), h.tdb.db().clone()));

    // The record: 200, with the flag that says the text is withheld rather than missing.
    let (status, body) = get(&app, &format!("/api/v1/works/{RESTRICTED}")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["title"], "Held close");
    assert_eq!(
        body["content_withheld"], true,
        "a stranger must be told the text exists and is withheld, not left to think the link is broken: {body}"
    );
    let rendered = body.to_string();
    assert!(
        !rendered.contains("The body nobody signed in for"),
        "no unit text may reach a stranger: {rendered}"
    );

    // The body: refused, and the refusal says *sign in* rather than *not found* — because the reader was just handed
    // the record, so "this work does not exist" would be a lie. Indistinguishability protects works whose existence
    // is itself hidden; a locked work is not one of them.
    let (status, refusal) = get(&app, &format!("/works/{RESTRICTED}/units/{UNIT}")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{refusal}");
    assert_eq!(refusal["error"]["code"], "AUTH_REQUIRED");

    // A private work keeps the old rule: the record and the body are both absent, and both answer as a nonexistent
    // work does, so the status cannot be used to enumerate what exists.
    let (status, _) = get(&app, &format!("/api/v1/works/{PRIVATE}")).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a private work is not a record"
    );
    let (private_body, _) = get(&app, &format!("/works/{PRIVATE}/units/{PRIVATE_UNIT}")).await;
    let (absent_body, _) = get(
        &app,
        "/works/01957000-0000-4000-8000-0000000000ff/units/01956000-0000-4000-8000-0000000000d2",
    )
    .await;
    assert_eq!(
        private_body, absent_body,
        "a private work and a work that never existed must be indistinguishable"
    );
}
