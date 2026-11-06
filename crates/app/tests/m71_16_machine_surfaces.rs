//! M71-16 — machine surfaces obey the same eligibility rules as pages.
//!
//! The failure this guards is quiet: a sitemap that lists an unlisted work turns "reachable by link" into "indexed by
//! Google", and that is a different promise than the one the visibility setting made. A sitemap is enumeration wearing
//! a machine's clothes.
//!
//! Asserts:
//! 1. `/sitemap.xml` is XML, lists `/`, the published public work, and its tag;
//! 2. it does not list an unlisted, private or restricted work, nor a tag that only those works carry;
//! 3. the body is the same for a signed-in reader — no personalisation, which is what `Audience::Anonymous` claims;
//! 4. an operator's base URL with `&` in it cannot make the document unparseable.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use darkhold_app::config::Config;
use darkhold_app::server::{self, set_trust_proxy};
use darkhold_app::state::AppState;
use darkhold_db::DatabaseConfig;
use std::path::{Path, PathBuf};
use tower::ServiceExt;

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-m7116-{tag}-{}-{:?}",
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

const ACCOUNT: &str = "01955000-0000-4000-8000-0000000005a1";
const PSEUD: &str = "01955000-0000-4000-8000-0000000005e1";
const PUBLIC: &str = "01957000-0000-4000-8000-0000000005b1";
const UNLISTED: &str = "01957000-0000-4000-8000-0000000005b2";
const PRIVATE: &str = "01957000-0000-4000-8000-0000000005b3";
const RESTRICTED: &str = "01957000-0000-4000-8000-0000000005b4";
const OPEN_TAG: &str = "01954000-0000-4000-8000-0000000005f1";
const HIDDEN_TAG: &str = "01954000-0000-4000-8000-0000000005f2";

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
        server::build_router(AppState::new(
            self.config.clone(),
            self.tdb.db().clone(),
        ))
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
             VALUES ('{ACCOUNT}','active','sitemap@example.invalid','adult',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ))
        .await;
        self.exec(&format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at,
                                 updated_at, version)
             VALUES ('{PSEUD}','{ACCOUNT}','sitemapwriter','Sitemap Writer','listed',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ))
        .await;
        for (id, name) in [(OPEN_TAG, "open trope"), (HIDDEN_TAG, "secret trope")] {
            self.exec(&format!(
                "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at)
                 VALUES ('{id}','tag','{name}','{name}','2026-01-01T00:00:00Z')"
            ))
            .await;
        }

        for (id, title, visibility) in [
            (PUBLIC, "Open Work", "public"),
            (UNLISTED, "Unlisted Work", "unlisted"),
            (PRIVATE, "Private Work", "private"),
            (RESTRICTED, "Restricted Work", "restricted"),
        ] {
            self.exec(&format!(
                "INSERT INTO works (id, owner_pseud_id, title, language, rating, visibility, lifecycle,
                                    created_at, updated_at, version)
                 VALUES ('{id}','{PSEUD}','{title}','en','general','{visibility}','published',
                         '2026-01-01T00:00:00Z','2026-01-02T00:00:00Z',1)"
            ))
            .await;
        }
        // The open tag rides the public work; the hidden tag rides only the unlisted one.
        self.exec(&format!(
            "INSERT INTO work_tags (work_id, node_id, prominence, added_at)
             VALUES ('{PUBLIC}','{OPEN_TAG}',1,'2026-01-01T00:00:00Z')"
        ))
        .await;
        self.exec(&format!(
            "INSERT INTO work_tags (work_id, node_id, prominence, added_at)
             VALUES ('{UNLISTED}','{HIDDEN_TAG}',1,'2026-01-01T00:00:00Z')"
        ))
        .await;
    }
}

async fn raw(app: axum::Router, uri: &str) -> Response {
    app.oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn body_of(response: Response) -> (StatusCode, String, String) {
    let status = response.status();
    let content_type = response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, content_type, String::from_utf8_lossy(&bytes).into_owned())
}

#[tokio::test]
async fn the_sitemap_lists_what_a_crawler_may_index_and_nothing_else() {
    let harness = Harness::new("eligibility").await;
    harness.seed().await;

    let (status, content_type, body) = body_of(raw(harness.client(), "/sitemap.xml").await).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        content_type.starts_with("application/xml"),
        "a sitemap announced as anything else is fetched and mis-parsed: {content_type}"
    );
    assert!(body.contains("<urlset"), "{body}");

    assert!(body.contains(&format!("/works/{PUBLIC}")), "{body}");
    assert!(body.contains("/tags/open%20trope"), "the tag is a page: {body}");

    for (id, why) in [
        (UNLISTED, "unlisted means reachable by link, not by enumeration"),
        (PRIVATE, "private is not published to anyone"),
        (RESTRICTED, "restricted is absent from the anonymous door and so from here"),
    ] {
        assert!(
            !body.contains(&format!("/works/{id}")),
            "{why}; found /works/{id} in {body}"
        );
    }
    assert!(
        !body.contains("/tags/secret%20trope"),
        "a tag whose only work is unlisted is a 404 and teaches a crawler to distrust the file: {body}"
    );

    // The document parses: no unescaped ampersand, no stray bracket.
    let opening = body.matches("<url>").count();
    let closing = body.matches("</url>").count();
    assert_eq!(opening, closing, "unbalanced entries in {body}");
}

#[tokio::test]
async fn the_sitemap_is_the_same_file_for_a_signed_in_reader() {
    let harness = Harness::new("personal").await;
    harness.seed().await;

    let (_, _, anonymous) = body_of(raw(harness.client(), "/sitemap.xml").await).await;
    // A session cookie that means nothing here: the route takes no session layer, and that is the point. The assertion
    // is that the response cannot differ, not that a particular cookie was ignored.
    let request = Request::builder()
        .uri("/sitemap.xml")
        .header("cookie", "darkhold_session=not-a-real-session")
        .body(Body::empty())
        .unwrap();
    let response = harness.client().oneshot(request).await.unwrap();
    let (status, _, personalised) = body_of(response).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        anonymous, personalised,
        "a machine surface that varies by caller is a privacy fault, not a feature"
    );
}

#[tokio::test]
async fn a_base_url_with_an_ampersand_still_produces_a_parseable_document() {
    let mut harness = Harness::new("escape").await;
    harness.seed().await;
    harness.config.site.base_url = "https://example.invalid/?a=1&b=2".to_owned();

    let (status, _, body) = body_of(raw(harness.client(), "/sitemap.xml").await).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.contains("&amp;b=2"),
        "an unescaped ampersand makes the whole document unparseable: {body}"
    );
    assert!(
        !body.contains("a=1&b=2"),
        "the raw ampersand survived: {body}"
    );
}
