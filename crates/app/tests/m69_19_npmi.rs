//! M69-19 — related tags are ranked by NPMI, not by raw co-occurrence.
//!
//! Raw counts reward ubiquitous tags: a tag that appears on every work co-occurs with everything and therefore relates
//! to nothing. NPMI divides the co-occurrence by both tags' own prevalence, so a rare tag that always travels with the
//! query tag outranks a common one that merely happens to be present.
//!
//! The fixture is built so the two rankings **disagree**, which is the only way this test can be evidence:
//!
//! ```text
//! 10 works;  N = work-tag links = 18  (lonely adds one on w10 and relates to nothing: every pair it forms is seen once)
//!   w1..w3: query + common + rare       (3 links each)
//!   w4:     query + common
//!   w5..w9: common
//!   w10:    common + lonely
//!
//!   query  appears in 4 works   P(query) = 4/18
//!   common appears in 10 works  P(common) = 10/18,  P(query,common) = 4/18
//!   rare   appears in 3 works   P(rare)   = 3/18,   P(query,rare)   = 3/18
//!
//!   npmi(query, common) = ln((4/18)/((4/18)(10/18))) / -ln(4/18) = 0.3908
//!   npmi(query, rare)   = ln((3/18)/((4/18)( 3/18))) / -ln(3/18) = 0.8394
//! ```
//!
//! Raw co-occurrence puts `common` first (4 > 3). NPMI puts `rare` first. Asserting the order, with both numbers, is
//! what makes a regression to counting visible.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server::{self, set_trust_proxy};
use darkhold_app::state::AppState;
use darkhold_db::DatabaseConfig;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tower::ServiceExt;

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-npmi-{tag}-{}-{:?}",
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
        sqlx::query(statement)
            .execute(self.tdb.db().sqlite_pool().expect("sqlite"))
            .await
            .unwrap_or_else(|error| panic!("{statement}\n{error}"));
    }

    async fn seed(&self) {
        self.exec(&format!(
            "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version)
             VALUES ('{ACCOUNT}','active','npmi@example.invalid','adult',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ))
        .await;
        self.exec(&format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at,
                                 updated_at, version)
             VALUES ('{PSEUD}','{ACCOUNT}','npmiwriter','NPMI Writer','listed',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ))
        .await;

        // Three tags, with node ids derived from their names for readability in a failure.
        for (node, name) in [
            ("01954000-0000-4000-8000-0000000003f1", "query"),
            ("01954000-0000-4000-8000-0000000003f2", "common"),
            ("01954000-0000-4000-8000-0000000003f3", "rare"),
            ("01954000-0000-4000-8000-0000000003f4", "lonely"),
        ] {
            self.exec(&format!(
                "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at)
                 VALUES ('{node}','tag','{name}','{name}','2026-01-01T00:00:00Z')"
            ))
            .await;
        }

        for index in 1..=10 {
            let work = format!("01957000-0000-4000-8000-0000000004{index:02}");
            self.exec(&format!(
                "INSERT INTO works (id, owner_pseud_id, title, language, rating, visibility, lifecycle,
                                    created_at, updated_at, version)
                 VALUES ('{work}','{PSEUD}','Work {index}','en','general','public','published',
                         '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
            ))
            .await;
            let mut tags = vec!["01954000-0000-4000-8000-0000000003f2"]; // common: every work
            if index <= 4 {
                tags.push("01954000-0000-4000-8000-0000000003f1"); // query: w1..w4
            }
            if index <= 3 {
                tags.push("01954000-0000-4000-8000-0000000003f3"); // rare: w1..w3
            }
            if index == 10 {
                // One work only: every pair it forms is seen once, and a pair seen once is not evidence.
                tags.push("01954000-0000-4000-8000-0000000003f4");
            }
            for node in tags {
                self.exec(&format!(
                    "INSERT INTO work_tags (work_id, node_id, prominence, added_at)
                     VALUES ('{work}','{node}',1,'2026-01-01T00:00:00Z')"
                ))
                .await;
            }
        }
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
async fn a_rare_companion_outranks_a_common_one() {
    let harness = Harness::new("order").await;
    harness.seed().await;

    let (status, body) = get(harness.client(), "/api/v1/tags/query/related").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let parsed: Value = serde_json::from_str(&body).expect("json");
    let items = parsed["items"].as_array().expect("items");
    assert_eq!(
        items.len(),
        2,
        "a pair seen once is noise and is excluded: {body}"
    );

    assert_eq!(
        items[0]["tag"], "rare",
        "NPMI ranks the rare companion first: {body}"
    );
    assert_eq!(
        items[1]["tag"], "common",
        "raw counts would have put this first: {body}"
    );

    // The numbers are the evidence, not a decoration: 0.8394 vs 0.3908 is the disagreement with a raw count.
    let npmi = |index: usize| items[index]["npmi"].as_f64().expect("npmi");
    assert!((npmi(0) - 0.8394).abs() < 1e-3, "got {}", npmi(0));
    assert!((npmi(1) - 0.3908).abs() < 1e-3, "got {}", npmi(1));

    // And the raw counts are the opposite way round, which is the whole point of the row.
    assert_eq!(items[0]["cooccurrences"], 3);
    assert_eq!(items[1]["cooccurrences"], 4);
}

#[tokio::test]
async fn the_tag_page_carries_the_related_section_or_omits_it() {
    let harness = Harness::new("page").await;
    harness.seed().await;

    let (status, body) = get(harness.client(), "/tags/query").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("Related tags"), "{body}");
    assert!(
        body.contains("/tags/rare") && body.contains("/tags/common"),
        "the related tags are links: {body}"
    );
    test_support::assert_no_empty_section(&body, "tag page with related tags");

    // `lonely` is on one work, so every pair it forms is seen once and filtered: its page renders no section at all —
    // absent, not empty.
    let (status, body) = get(harness.client(), "/tags/lonely").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        !body.contains("Related tags"),
        "a tag with nothing related renders no heading: {body}"
    );
    test_support::assert_no_empty_section(&body, "tag page without related tags");
}
