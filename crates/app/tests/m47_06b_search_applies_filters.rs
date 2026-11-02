//! M47-06b — the search endpoint honours the filters it reports.
//!
//! `GET /api/v1/search` resolves `min_words`, `max_words`, `sort` and `max_rating` from the query
//! string or the reader's stored `search_settings`, returns them in the response's `filters`
//! object — and, before this file existed, never passed one of them to the query. The endpoint
//! reported a limit it did not apply, which is worse than not offering it: the reader sets
//! "maximum rating: general" in content preferences, sees `"max_rating": "general"` in the
//! response, and still gets explicit works.
//!
//! Each test below is written so that it passes **only** if the value reached the statement.
//! They are deliberately assertions about which works are *absent*: asserting the echoed
//! `filters` object is what `m47_05_search_defaults.rs` already does, and that test stayed green
//! through the whole period in which the filters did nothing.
//!
//! Both engines: `TestDb` is SQLite by default and PostgreSQL under `DARKHOLD_TEST_PG_URL`. The
//! rating rank is a `CASE` over the stored text and the word bound reads the same aggregate the
//! response reports, so a dialect that disagrees shows up here.

use std::path::PathBuf;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::content::create_work;
use darkhold_db::identity::{create_account, create_pseud, AccountStatus};
use darkhold_db::search::rebuild_work_index;
use darkhold_domain::ids::{PseudId, WorkId};
use darkhold_domain::policy::AgeState;
use serde_json::Value;
use test_support::TestDb;
use tower::ServiceExt;
use uuid::Uuid;

/// Every fixture work's body, so one query string finds all of them and only the filter can
/// change the answer.
const FINDABLE: &str = "commonword";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-m47-06-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

struct Harness {
    tdb: TestDb,
    dir: PathBuf,
}

impl Harness {
    async fn new(tag: &str) -> Self {
        let dir = scratch_dir(tag);
        let tdb = TestDb::connect_with_dir(tag, &dir).await;
        Self { tdb, dir }
    }

    fn db(&self) -> &darkhold_db::Database {
        self.tdb.db()
    }

    /// The router the tests call. Built over the database `TestDb` already connected, so this
    /// suite runs on both engines; the config's own database URL is unused once the handle is
    /// passed to `AppState`.
    fn router(&self) -> axum::Router {
        let mut config = Config::development_defaults();
        config.storage.root = self.dir.clone();
        server::build_router(AppState::new(config, self.db().clone()))
    }

    async fn exec(&self, query: &str) {
        let q = self.tdb.sql(query);
        match self.db().backend() {
            darkhold_db::Backend::Sqlite => {
                sqlx::query(&q)
                    .execute(self.db().sqlite_pool().expect("sqlite"))
                    .await
                    .expect("exec");
            }
            darkhold_db::Backend::Postgres => {
                sqlx::query(&q)
                    .execute(self.db().postgres_pool().expect("pg"))
                    .await
                    .expect("exec");
            }
        }
    }

    async fn pseud(&self, handle: &str) -> PseudId {
        let account = create_account(
            self.db(),
            &format!("m47-06-{}@example.test", Uuid::new_v4()),
            AgeState::DeclaredAdult,
            AccountStatus::Active,
        )
        .await
        .expect("create_account");
        create_pseud(self.db(), account, handle, handle)
            .await
            .expect("create_pseud")
    }

    /// A published, public work of `words` words, rated `rating`, last touched at `updated_at`,
    /// findable by [`FINDABLE`] — `hits` times, because `score` is the number of matching term
    /// rows and the ordering tests need it to disagree with recency.
    async fn work(
        &self,
        owner: PseudId,
        title: &str,
        rating: &str,
        words: i64,
        updated_at: &str,
        hits: i64,
    ) -> WorkId {
        let work = create_work(self.db(), owner, title, None)
            .await
            .expect("create_work")
            .id;
        let unit = Uuid::new_v4();
        let revision = Uuid::new_v4();
        // `units.current_revision_id` and `revisions.unit_id` form a reference cycle: the unit is
        // inserted with a NULL pointer, the revision is added, the pointer is set last.
        self.exec(&format!(
            "INSERT INTO units \
             (id, work_id, order_key, title, current_revision_id, created_at, updated_at) \
             VALUES ('{unit}', '{work}', 1, 'Unit', NULL, \
             '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        ))
        .await;
        self.exec(&format!(
            "INSERT INTO revisions \
             (id, unit_id, revision_number, document_json, sanitized_html, plain_text, \
              word_count, created_by_pseud_id, created_at) \
             VALUES ('{revision}', '{unit}', 1, '{{}}', '', '', {words}, '{owner}', \
             '2026-01-01T00:00:00Z')"
        ))
        .await;
        self.exec(&format!(
            "UPDATE units SET current_revision_id = '{revision}' WHERE id = '{unit}'"
        ))
        .await;
        self.exec(&format!(
            "UPDATE works SET lifecycle = 'published', visibility = 'public', \
             rating = '{rating}', updated_at = '{updated_at}' WHERE id = '{work}'"
        ))
        .await;
        let body = vec![FINDABLE; hits as usize].join(" ");
        rebuild_work_index(self.db(), &work, &body)
            .await
            .expect("rebuild index");
        work
    }

    /// `GET /api/v1/search?q={expression}`, with the expression percent-encoded by the caller,
    /// returning `(status, body)`.
    ///
    /// Separate from [`Harness::search`] because a query-language expression is not a filter
    /// parameter: the `q` value *is* the query here, and appending it to `FINDABLE` would test the
    /// old path instead of this one.
    async fn search_q(&self, expression: &str) -> (StatusCode, Value) {
        let resp = self
            .router()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/search?q={expression}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        let body = axum::body::to_bytes(resp.into_body(), 10_000_000)
            .await
            .unwrap();
        (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
    }

    /// `GET /api/v1/search?{query}`, returning `(status, body)`.
    async fn search(&self, query: &str) -> (StatusCode, Value) {
        let resp = self
            .router()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/search?q={FINDABLE}&{query}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        let body = axum::body::to_bytes(resp.into_body(), 10_000_000)
            .await
            .unwrap();
        (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
    }
}

/// **A relative date bound reaches the statement, and a missing colon is named.**
///
/// `updated>=-7d` is the spelling the idea sweep that asked for this feature wrote, and it used to
/// be refused outright ("takes a YYYY-MM-DD date"). Now the window grammar resolves the age, and
/// the colon-less spelling is diagnosed instead of silently becoming free text that matches
/// nothing — the failure mode where a reader sees an empty page and concludes there is no work.
///
/// The fixtures are dated March 2026; the bounds are ten years, so the assertions stay true for a
/// decade rather than depending on the day the suite runs.
#[tokio::test]
async fn a_relative_date_bound_reaches_the_statement() {
    let h = Harness::new("relative-date").await;
    three_works(&h).await;

    // Ten years back includes every fixture...
    let (status, body) = h.search_q("updated%3A%3E%3D-3650d").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(
        titles(&body).len(),
        3,
        "a ten-year age bound must include the March fixtures: {body}"
    );

    // ...and "ten years ago or earlier" includes none of them, so the bound is doing work rather
    // than matching everything.
    let (status, body) = h.search_q("updated%3A%3C%3D-3650d").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert!(
        titles(&body).is_empty(),
        "a ten-year-old ceiling must exclude the March fixtures: {body}"
    );
}

/// The colon-less spelling is a `422` that names the colon form, not an empty page.
#[tokio::test]
async fn a_comparison_without_its_colon_is_refused_with_the_fix() {
    let h = Harness::new("no-colon").await;
    three_works(&h).await;

    let (status, body) = h.search_q("updated%3E%3D-3650d").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "body: {body}");
    let message = body["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("updated:>=-3650d"),
        "the refusal must name the colon form, got: {message}"
    );
}

/// The titles the response returned, in the order it returned them.
fn titles(body: &Value) -> Vec<String> {
    body["items"]
        .as_array()
        .expect("items is an array")
        .iter()
        .map(|item| item["title"].as_str().unwrap_or_default().to_owned())
        .collect()
}

/// Three works, one of each size and rating, all findable by the same term:
/// `Short` 2k `general`, `Long` 40k `mature`, `Mid` 8k `explicit`.
/// `Short` 2k `general` (3 hits), `Long` 40k `mature` (1 hit), `Mid` 8k `explicit` (2 hits).
///
/// The scores and the timestamps are deliberately in opposite orders, so the default ordering
/// (`score DESC`) and `sort=newest` cannot coincide — a fixture where they agree lets a test for
/// the sort pass against a search that ignores it, which is exactly what the first version of
/// `sort_newest_orders_by_update` did.
async fn three_works(h: &Harness) -> (WorkId, WorkId, WorkId) {
    let owner = h.pseud("author").await;
    let short = h
        .work(owner, "Short", "general", 2_000, "2026-03-01T00:00:00Z", 3)
        .await;
    let long = h
        .work(owner, "Long", "mature", 40_000, "2026-03-03T00:00:00Z", 1)
        .await;
    let mid = h
        .work(owner, "Mid", "explicit", 8_000, "2026-03-02T00:00:00Z", 2)
        .await;
    (short, long, mid)
}

/// **`max_words` is absent from the response's items, not just from its `filters`.**
///
/// Before M47-06b this returned all three: the value was resolved, echoed back as
/// `"max_words": 10000`, and never reached the statement.
#[tokio::test]
async fn max_words_is_not_a_lie() {
    let h = Harness::new("max-words").await;
    three_works(&h).await;

    let (status, body) = h.search("max_words=10000").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(
        body["filters"]["max_words"], 10_000,
        "the echo is unchanged"
    );

    let found = titles(&body);
    assert!(
        found.contains(&"Short".to_owned()) && found.contains(&"Mid".to_owned()),
        "a 2k and an 8k work both fit under 10k: {found:?}"
    );
    assert!(
        !found.contains(&"Long".to_owned()),
        "a 40k work must not survive max_words=10000, and it did: {found:?}"
    );
}

/// **`min_words` excludes the short work.** The other direction of the same predicate, because a
/// bound written as `>=` for the maximum would pass the test above and fail this one.
#[tokio::test]
async fn min_words_is_not_a_lie() {
    let h = Harness::new("min-words").await;
    three_works(&h).await;

    let (status, body) = h.search("min_words=5000").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");

    let found = titles(&body);
    assert!(
        !found.contains(&"Short".to_owned()),
        "a 2k work must not survive min_words=5000: {found:?}"
    );
    assert!(
        found.contains(&"Long".to_owned()) && found.contains(&"Mid".to_owned()),
        "a 40k and an 8k work both clear 5k: {found:?}"
    );
}

/// **`max_rating` excludes above the ceiling and keeps what is at or below it.**
///
/// The rank is a `CASE` over the stored text — `general` 0, `teen` 1, `mature` 2, else 3 — so this
/// also pins the absent-and-unknown direction: a rating the instance later adds ranks 3 and is
/// therefore hidden by a `general` ceiling rather than leaking past it.
#[tokio::test]
async fn max_rating_is_not_a_lie() {
    let h = Harness::new("max-rating").await;
    three_works(&h).await;

    let (status, body) = h.search("max_rating=general").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");

    assert_eq!(
        titles(&body),
        vec!["Short".to_owned()],
        "only the general work may survive max_rating=general"
    );

    let (status, body) = h.search("max_rating=mature").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let found = titles(&body);
    assert!(
        found.contains(&"Long".to_owned()) && found.contains(&"Short".to_owned()),
        "mature admits mature and general: {found:?}"
    );
    assert!(
        !found.contains(&"Mid".to_owned()),
        "mature must not admit explicit: {found:?}"
    );
}

/// **`sort=newest` orders by `updated_at`, and the default orders by score.**
///
/// Both orders are asserted, because the first version of this test asserted only the `newest`
/// one against a fixture where `score DESC` produced the same sequence — it passed against a
/// search that ignored `sort` entirely. The two must be different for either assertion to mean
/// anything, so `three_works` gives the most recently updated work the *lowest* score.
#[tokio::test]
async fn sort_newest_orders_by_update() {
    let h = Harness::new("sort-newest").await;
    three_works(&h).await;

    let (status, body) = h.search("").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(
        titles(&body),
        vec!["Short".to_owned(), "Mid".to_owned(), "Long".to_owned()],
        "the default order is score-first: 3 hits, 2 hits, 1 hit"
    );

    let (status, body) = h.search("sort=newest").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(
        titles(&body),
        vec!["Long".to_owned(), "Mid".to_owned(), "Short".to_owned()],
        "newest first: Long (03-03), Mid (03-02), Short (03-01)"
    );
}

/// **An unrecognised `sort` falls back to the default order instead of failing.**
///
/// A stored default written by an older build must not make search a `422` for that reader, and a
/// silent 500 is worse. Falling back is the same answer the reader had before the value existed.
#[tokio::test]
async fn an_unknown_sort_falls_back_rather_than_failing() {
    let h = Harness::new("sort-unknown").await;
    three_works(&h).await;

    let (status, body) = h.search("sort=whatever-the-old-build-wrote").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(
        titles(&body).len(),
        3,
        "every work is still returned: {body}"
    );
}
