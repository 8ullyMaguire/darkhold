//! M69-09 — reader affinity end to end: the sweep, the second run, and the reader who has not been measured.
//!
//! The arithmetic has unit tests in `darkhold-domain`. These tests run the store and the doors, and they are built around
//! the two claims that a score like this gets wrong when nobody looks:
//!
//! * a second sweep over unchanged data reports **zeros** (an upsert that "changed" again every run is a sweep that
//!   matches nothing and writes a new row), and
//! * a reader with no evidence has **no row and no score**, because "0% aligned" is a judgement the instance never made.
//!
//! The evidence is seeded directly — bookmarks, operator affinities, work tags and arena ballots — because the sweep is
//! the subject here, not the bookmarking or the arena.

use std::path::PathBuf;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use serde_json::{json, Value};
use test_support::TestDb;
use tower::ServiceExt;

const PASSWORD: &str = "test-pass-1234";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-m6909-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

struct Harness {
    _dir: PathBuf,
    tdb: TestDb,
    config: Config,
}

impl Harness {
    async fn new(tag: &str) -> Self {
        server::set_trust_proxy(false);
        let dir = scratch_dir(tag);
        let tdb = TestDb::connect_with_dir(tag, &dir).await;
        let mut config = Config::development_defaults();
        config.storage.root = dir.clone();
        Self {
            _dir: dir,
            tdb,
            config,
        }
    }

    fn client(&self) -> Client {
        Client::new(server::build_router(AppState::new(
            self.config.clone(),
            self.tdb.db().clone(),
        )))
    }

    /// A work with tags, owned by a pseud, so bookmarks and tag vectors have something real to point at.
    async fn work(&self, title: &str, tags: &[&str]) -> String {
        let now = "2026-11-07T00:00:00Z";
        let pseud = self
            .first_pseud()
            .await
            .expect("an account registered before seeding works");
        let owner: darkhold_domain::ids::PseudId = pseud.parse().expect("pseud id parses");
        // The product's own constructor rather than an INSERT: `works` has fifteen NOT NULL columns with defaults that
        // belong to the store, and a test that spells them out is a test that breaks when one is added.
        let work = darkhold_db::content::create_work(self.tdb.db(), owner, title, None)
            .await
            .expect("create work");
        let id = work.id.to_string();
        for tag in tags {
            let node = format!("node-{tag}");
            sqlx::query(
                // No `weight` column: 0103 rebuilt `work_tags` and the live shape is (work_id, node_id, added_at, …).
                "INSERT OR IGNORE INTO work_tags (work_id, node_id, added_at) VALUES (?, ?, ?)",
            )
            .bind(&id)
            .bind(&node)
            .bind(now)
            .execute(self.tdb.db().sqlite_pool().expect("sqlite"))
            .await
            .expect("insert tag");
        }
        id
    }

    async fn first_pseud(&self) -> Option<String> {
        sqlx::query_scalar::<_, String>("SELECT id FROM pseuds ORDER BY created_at LIMIT 1")
            .fetch_optional(self.tdb.db().sqlite_pool().expect("sqlite"))
            .await
            .expect("pseud lookup")
    }
}

struct Client {
    app: axum::Router,
    cookies: Vec<(String, String)>,
}

impl Client {
    fn new(app: axum::Router) -> Self {
        Self {
            app,
            cookies: Vec::new(),
        }
    }

    fn capture(&mut self, response: &axum::response::Response) {
        for value in response.headers().get_all(header::SET_COOKIE) {
            let Ok(text) = value.to_str() else { continue };
            let Some((pair, _)) = text.split_once(';') else {
                continue;
            };
            let Some((name, value)) = pair.split_once('=') else {
                continue;
            };
            let name = name.trim().to_owned();
            let value = value.trim().to_owned();
            self.cookies.retain(|(k, _)| k != &name);
            if !value.is_empty() {
                self.cookies.push((name, value));
            }
        }
    }

    async fn send(&mut self, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut builder = Request::builder()
            .method(Method::from_bytes(method.as_bytes()).expect("method"))
            .uri(uri);
        if body.is_some() {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
        }
        if !self.cookies.is_empty() {
            let cookie = self
                .cookies
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect::<Vec<String>>()
                .join("; ");
            builder = builder.header(header::COOKIE, cookie);
        }
        if let Some(token) = self
            .cookies
            .iter()
            .find(|(k, _)| k == "darkhold_csrf")
            .map(|(_, v)| v.to_owned())
        {
            builder = builder.header("x-csrf-token", token);
        }
        let body = body.map_or_else(Body::empty, |value| Body::from(value.to_string()));
        let response = self
            .app
            .clone()
            .oneshot(builder.body(body).expect("request"))
            .await
            .expect("response");
        let status = response.status();
        self.capture(&response);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn register(&mut self, handle: &str) -> String {
        let (status, body) = self
            .send(
                "POST",
                "/api/v1/auth/register",
                Some(json!({
                    "email": format!("m69-09-{handle}@example.test"),
                    "password": PASSWORD,
                    "handle": handle,
                    "display_name": handle,
                    "age_band": "adult",
                })),
            )
            .await;
        assert!(
            status == StatusCode::CREATED || status == StatusCode::OK,
            "register {handle}: {body}"
        );
        let (status, me) = self.send("GET", "/api/v1/auth/me", None).await;
        assert_eq!(status, StatusCode::OK, "{me}");
        me["account"]["id"]
            .as_str()
            .or_else(|| me["id"].as_str())
            .expect("account id")
            .to_owned()
    }
}

/// Bookmark a work for an account, straight into the store: the sweep is the subject, not the bookmark route.
async fn bookmark(h: &Harness, account: &str, work: &str) {
    sqlx::query(
        "INSERT INTO bookmarks (id, account_id, subject_type, subject_id, note, is_public, created_at, updated_at)
         VALUES (?, ?, 'work', ?, '', 0, '2026-11-07T00:00:00Z', '2026-11-07T00:00:00Z')",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(account)
    .bind(work)
    .execute(h.tdb.db().sqlite_pool().expect("sqlite"))
    .await
    .expect("insert bookmark");
}

/// The operator's opinion of a work.
async fn value_work(h: &Harness, work: &str, bp: i64) {
    sqlx::query(
        "INSERT INTO operator_affinities (work_id, affinity_bp, operator, rationale, set_at)
         VALUES (?, ?, 'operator', 'seed', '2026-11-07T00:00:00Z')",
    )
    .bind(work)
    .bind(bp)
    .execute(h.tdb.db().sqlite_pool().expect("sqlite"))
    .await
    .expect("insert operator affinity");
}

#[tokio::test]
async fn an_aligned_reader_scores_high_a_stranger_low_and_a_silent_reader_has_no_score_at_all() {
    let mut h = Harness::new("scoring").await;
    let mut op = h.client();
    let operator = op.register("the-operator").await;
    h.config.administration.operator_account_id = Some(operator.parse().expect("account id"));

    // The instance values two works that share a tag.
    let liked = h.work("The good one", &["angst", "slowburn"]).await;
    let also_liked = h.work("Another good one", &["angst"]).await;
    value_work(&h, &liked, 8_000).await;
    value_work(&h, &also_liked, 6_000).await;
    // A work the operator shrugs at is not "valued": 1bp is not an opinion.
    let shrugged = h.work("Meh", &["fluff"]).await;
    value_work(&h, &shrugged, 1).await;

    let mut aligned = h.client();
    let aligned_id = aligned.register("aligned").await;
    bookmark(&h, &aligned_id, &liked).await;
    bookmark(&h, &aligned_id, &also_liked).await;

    let mut stranger = h.client();
    let stranger_id = stranger.register("stranger").await;
    let unliked = h.work("Not for them", &["fluff"]).await;
    bookmark(&h, &stranger_id, &unliked).await;

    // A reader who has registered and done nothing: no bookmarks, no ballots.
    let mut silent = h.client();
    silent.register("silent").await;

    // Nobody is scored before the sweep runs: this is a stored reading, not a request-time computation.
    let (_, body) = aligned.send("GET", "/api/v1/me/affinity", None).await;
    assert_eq!(
        body["scored"],
        json!(false),
        "the score is a row, not a calculation per page: {body}"
    );

    let mut operator_client = h.client();
    operator_client.cookies = op.cookies.clone();
    let (status, body) = operator_client
        .send(
            "POST",
            "/api/v1/operator/affinity/recompute",
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["initialised"],
        json!(2),
        "two readers had evidence: {body}"
    );
    assert_eq!(body["changed"], json!(0), "{body}");
    assert_eq!(
        body["unscored"],
        json!(0),
        "the sweep starts from the evidence tables, so a reader who has done nothing is not a candidate at all -- \
         they are not scored, and they are not counted as unscorable either: {body}"
    );

    let (_, me) = aligned.send("GET", "/api/v1/me/affinity", None).await;
    assert_eq!(me["scored"], json!(true), "{me}");
    let aligned_score = me["affinity"]["score_bp"].as_i64().expect("score");
    assert!(
        aligned_score >= 5_000,
        "a reader who bookmarked both valued works is at least half aligned: {me}"
    );

    let (_, me) = stranger.send("GET", "/api/v1/me/affinity", None).await;
    let stranger_score = me["affinity"]["score_bp"].as_i64().expect("score");
    assert!(
        stranger_score < aligned_score,
        "a reader who collected a work the operator shrugged at must not score above one who collected the valued ones: {me}"
    );

    let (_, me) = silent.send("GET", "/api/v1/me/affinity", None).await;
    assert_eq!(
        me["scored"],
        json!(false),
        "a reader with no evidence has no score, and '0% aligned' would be a judgement never made: {me}"
    );

    // And the instance's ranking is not the reader's: a plain reader is refused it.
    let (status, body) = aligned.send("GET", "/api/v1/operator/affinity", None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
}

#[tokio::test]
async fn a_second_sweep_over_unchanged_data_reports_zeros() {
    let mut h = Harness::new("idempotent").await;
    let mut op = h.client();
    let operator = op.register("the-operator").await;
    h.config.administration.operator_account_id = Some(operator.parse().expect("account id"));

    let liked = h.work("Valued", &["angst"]).await;
    value_work(&h, &liked, 9_000).await;

    let mut reader = h.client();
    let reader_id = reader.register("reader").await;
    bookmark(&h, &reader_id, &liked).await;

    let mut operator_client = h.client();
    operator_client.cookies = op.cookies.clone();

    let (status, first) = operator_client
        .send(
            "POST",
            "/api/v1/operator/affinity/recompute",
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["initialised"], json!(1), "{first}");

    // The same data, swept again. The upsert repairs one row; the report must say so rather than claiming a change.
    let (status, second) = operator_client
        .send(
            "POST",
            "/api/v1/operator/affinity/recompute",
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["initialised"], json!(0), "{second}");
    assert_eq!(
        second["changed"],
        json!(0),
        "an upsert that reports a change every run is a sweep matching nothing: {second}"
    );
    assert_eq!(second["unchanged"], json!(1), "{second}");

    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM reader_affinity")
        .fetch_one(h.tdb.db().sqlite_pool().expect("sqlite"))
        .await
        .expect("count");
    assert_eq!(
        rows, 1,
        "one reader is one row, however many times the sweep runs"
    );

    // New evidence is absorbed by the next run — a repair, not a one-shot.
    let more = h.work("Also valued", &["angst"]).await;
    value_work(&h, &more, 9_000).await;
    let (_, third) = operator_client
        .send(
            "POST",
            "/api/v1/operator/affinity/recompute",
            Some(json!({})),
        )
        .await;
    assert_eq!(third["initialised"], json!(0), "{third}");
    assert_eq!(
        third["changed"],
        json!(1),
        "new evidence must reach the stored score: a `DO NOTHING` upsert would pass every earlier assertion and \
         freeze this reader at their first reading: {third}"
    );
    let (_, me) = reader.send("GET", "/api/v1/me/affinity", None).await;
    assert!(
        me["affinity"]["score_bp"].as_i64().expect("score") < 10_000,
        "the reader now shares the instance's tag interest with two works instead of one, so the overlap fell: {me}"
    );
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM reader_affinity")
        .fetch_one(h.tdb.db().sqlite_pool().expect("sqlite"))
        .await
        .expect("count");
    assert_eq!(
        rows, 1,
        "a repair run does not add a second row per reader: {third}"
    );
}

#[tokio::test]
async fn an_instance_with_no_operator_scores_nobody_rather_than_scoring_everybody_zero() {
    // The "on but unconfigured" state. An instance with no designated operator has nothing for a reader to be close to,
    // and a sweep that answered 0 for the whole readership would be describing a judgement as a fact.
    let h = Harness::new("no-operator").await;
    let mut reader = h.client();
    let reader_id = reader.register("reader").await;
    let work = h.work("Anything", &["angst"]).await;
    bookmark(&h, &reader_id, &work).await;
    value_work(&h, &work, 9_000).await;

    let mut op = h.client();
    op.register("not-the-operator").await;

    // The sweep still runs; it just has no operator to measure against.
    let report = darkhold_db::reader_affinity::recompute_all(
        h.tdb.db(),
        None,
        "2026-11-07T00:00:00Z",
        false,
    )
    .await
    .expect("sweep");
    assert_eq!(report.initialised, 0);
    assert_eq!(report.changed, 0);
    assert_eq!(
        report.unscored, 1,
        "the candidate is reported, not silently dropped"
    );
    assert!(!report.wrote());

    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM reader_affinity")
        .fetch_one(h.tdb.db().sqlite_pool().expect("sqlite"))
        .await
        .expect("count");
    assert_eq!(rows, 0, "no operator means no score, not a score of zero");
}
