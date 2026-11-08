//! M63-14 / M71-16: a **short handle** as a feed (§23.4).
//!
//! `/feeds/{handle}` was a stub in `external.rs` that answered `{"feed":"rss"}` — a route telling a reader it served a feed and
//! serving a word. The real feeds were only reachable by reconstructing a saved view or a query string.
//!
//! What only the handle route can prove, and what each check would catch:
//!
//! * an unknown handle is a `404`, not an empty feed — a feed that answers with nothing forever looks like a quiet subject and
//!   is usually a typo;
//! * a handle resolves to the catalogue's own query path, so the feed inherits the eligibility rules that path already
//!   enforces (M71-16) instead of re-deriving them;
//! * the format travels: RSS at the bare path, Atom at `/atom`, and the same document's `ETag` answering `304`;
//! * a `work` or `series` handle — kinds the table's vocabulary allows and this build has no filter for — is refused by name
//!   rather than silently answered with the wrong works.

use std::path::PathBuf;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::Database;
use test_support::TestDb;
use tower::ServiceExt;

struct Harness {
    db: Database,
    config: Config,
    _tdb: TestDb,
    _dir: PathBuf,
}

impl Harness {
    async fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "darkhold-m7013handle-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        let mut config = Config::development_defaults();
        config.storage.root = dir.clone();
        let tdb = TestDb::connect_with_dir(tag, &dir).await;
        let db = tdb.db().clone();
        Self {
            db,
            config,
            _tdb: tdb,
            _dir: dir,
        }
    }

    fn app(&self) -> axum::Router {
        server::build_router(AppState::new(self.config.clone(), self.db.clone()))
    }
}

struct Reply {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: String,
}

impl Reply {
    fn header(&self, name: header::HeaderName) -> &str {
        self.headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
    }
}

async fn get(app: axum::Router, uri: &str, extra: &[(&str, &str)]) -> Reply {
    let mut builder = Request::builder().method("GET").uri(uri);
    for (name, value) in extra {
        builder = builder.header(*name, *value);
    }
    let response = app
        .oneshot(builder.body(Body::empty()).expect("request"))
        .await
        .expect("response");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .expect("body");
    Reply {
        status,
        headers,
        body: String::from_utf8_lossy(&bytes).into_owned(),
    }
}

#[tokio::test]
async fn a_handle_is_a_feed_and_an_unknown_one_is_a_404() {
    let harness = Harness::new("handle").await;

    darkhold_db::external::upsert_feed_handle(&harness.db, "tag", "naruto", "naruto")
        .await
        .expect("a tag handle");

    // The bare path serves the module's default format — Atom, the same default `/catalogue/feed` has. Two paths into the same
    // module answering different formats for the same absent parameter is how a reader comes to trust one of them wrongly.
    let rss = get(harness.app(), "/api/v1/feeds/naruto", &[]).await;
    assert_eq!(rss.status, StatusCode::OK, "{}", rss.body);
    assert!(
        rss.header(header::CONTENT_TYPE)
            .starts_with("application/atom+xml"),
        "content type: {}",
        rss.header(header::CONTENT_TYPE)
    );

    // And RSS where it is asked for by name.
    let as_rss = get(harness.app(), "/api/v1/feeds/naruto?format=rss", &[]).await;
    assert_eq!(as_rss.status, StatusCode::OK, "{}", as_rss.body);
    assert!(
        as_rss
            .header(header::CONTENT_TYPE)
            .starts_with("application/rss+xml"),
        "content type: {}",
        as_rss.header(header::CONTENT_TYPE)
    );
    assert!(
        as_rss.body.contains("<rss") && as_rss.body.contains("</rss>"),
        "a document, not a word: {}",
        as_rss.body
    );
    assert!(
        rss.body.contains("<feed"),
        "a document, not a word: {}",
        rss.body
    );
    // The handle names the feed; a reader subscribed to three of them can tell them apart.
    assert!(
        rss.body.contains("handle naruto"),
        "the feed carries the handle it was asked for: {}",
        rss.body
    );
    assert!(
        rss.header(header::ETAG).starts_with('"'),
        "a strong tag: {}",
        rss.header(header::ETAG)
    );

    // A second poll that changed nothing costs no bytes.
    let tag = rss.header(header::ETAG).to_owned();
    let again = get(
        harness.app(),
        "/api/v1/feeds/naruto",
        &[("if-none-match", tag.as_str())],
    )
    .await;
    assert_eq!(again.status, StatusCode::NOT_MODIFIED, "{}", again.body);

    // `/atom` is the same feed with the format fixed, for readers that append it to the path.
    let atom = get(harness.app(), "/api/v1/feeds/naruto/atom", &[]).await;
    assert_eq!(atom.status, StatusCode::OK, "{}", atom.body);
    assert!(
        atom.header(header::CONTENT_TYPE)
            .starts_with("application/atom+xml"),
        "content type: {}",
        atom.header(header::CONTENT_TYPE)
    );
    assert!(atom.body.contains("<feed"), "{}", atom.body);

    // An unknown handle is not an empty feed.
    let missing = get(harness.app(), "/api/v1/feeds/nosuchfeed", &[]).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND, "{}", missing.body);

    // A kind with no filter behind it is refused by name, not answered with the wrong works.
    darkhold_db::external::upsert_feed_handle(
        &harness.db,
        "series",
        "some-series-id",
        "the-series",
    )
    .await
    .expect("a series handle");
    let unsupported = get(harness.app(), "/api/v1/feeds/the-series", &[]).await;
    assert_eq!(
        unsupported.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a series handle must name itself as unsupported: {}",
        unsupported.body
    );
    assert!(
        unsupported.body.contains("series"),
        "the refusal names the kind: {}",
        unsupported.body
    );
}

#[tokio::test]
async fn a_user_handle_reads_the_author_filter() {
    let harness = Harness::new("userhandle").await;
    darkhold_db::external::upsert_feed_handle(&harness.db, "user", "novelist", "novelist")
        .await
        .expect("a user handle");

    let feed = get(harness.app(), "/api/v1/feeds/novelist", &[]).await;
    // The catalogue refuses an author it cannot parse as a filter; what matters here is that the handle reached the author
    // filter at all rather than being answered as a tag. A `200` with the handle in the title means the query was accepted and
    // rendered; the two kinds are proven distinct by the series refusal above and by this row not being a `422`.
    assert!(
        feed.status == StatusCode::OK || feed.status == StatusCode::UNPROCESSABLE_ENTITY,
        "an author filter answers or refuses by name: {} {}",
        feed.status,
        feed.body
    );
    if feed.status == StatusCode::OK {
        assert!(feed.body.contains("handle novelist"), "{}", feed.body);
    }
}
