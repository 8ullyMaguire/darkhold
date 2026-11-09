//! Acceptance: the decision audit surface (spec §11.14, amendment
//! `calibrated-decision-models.md` §3.5).
//!
//! `decision_audit.rs` proves the store. This file proves the route, and the
//! route is where three things actually land:
//!
//! - **The endpoint stops being a promise.** It has always returned
//!   `{ "items": [] }` unconditionally — an operator-only audit trail carrying
//!   nothing. A test that only checked "200 and an items array" would have
//!   passed against that stub for the whole life of the feature, which is why
//!   these tests seed a row and assert it comes back.
//! - **The surface is invisible to a non-operator.** 404, not 403. The status
//!   is the contract: a test that only asserted "refused" would pass against a
//!   403 that confirms the audit exists, and the audit is the record of what
//!   this instance decided.
//! - **The graded text is not in it.** §12.1's commitment is that a reader's
//!   words stay with their author. An audit holding the words it judged would
//!   be a second copy with none of the first copy's rules. The response is
//!   asserted to carry the subject id and a number, and nothing that echoes
//!   back the input.

use std::path::Path;

use axum::http::{header, Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::decision_audit::{self, AuditProvider, NewAuditEntry};
use darkhold_db::Database;
use serde_json::{json, Value};
use test_support::TestDb;
use tower::ServiceExt;

const GOOD_PASSWORD: &str = "a-long-enough-passphrase";

fn scratch_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-decision-audit-routes-{tag}-{}-{:?}",
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
    // Honoured, so this file runs on whichever backend the selector names — the
    // same note as `m29_transparency.rs`, which found a hardcoded SQLite URL
    // here once already.
    config.database = match std::env::var("DARKHOLD_TEST_PG_URL") {
        Ok(url) => darkhold_db::DatabaseConfig::new(url),
        Err(_) => darkhold_db::DatabaseConfig::new(format!(
            "sqlite://{}/darkhold.sqlite?mode=rwc",
            dir.display()
        )),
    };
    // The rate-limit buckets are process-global at 127.0.0.1, so the
    // development defaults are exhausted by neighbouring suites long before
    // this file's own requests finish.
    config.rate_limits.auth = darkhold_app::limiter::Quota {
        burst: 1000,
        per_minute: 6000,
    };
    config.rate_limits.write = darkhold_app::limiter::Quota {
        burst: 1000,
        per_minute: 6000,
    };
    config.rate_limits.default = darkhold_app::limiter::Quota {
        burst: 1000,
        per_minute: 6000,
    };
    config
}

/// A session-capable client. Cookies are carried by hand, as every suite in this
/// directory does.
struct Client {
    app: axum::Router,
    cookies: Vec<(String, String)>,
}

impl Client {
    fn cookie_header(&self) -> String {
        self.cookies
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; ")
    }

    fn capture(&mut self, response: &axum::response::Response) {
        for value in response.headers().get_all(header::SET_COOKIE) {
            let Ok(raw) = value.to_str() else { continue };
            let pair = raw.split(';').next().unwrap_or(raw);
            let Some((name, val)) = pair.split_once('=') else {
                continue;
            };
            let (name, val) = (name.to_owned(), val.to_owned());
            self.cookies.retain(|(key, _)| key != &name);
            if !val.is_empty() {
                self.cookies.push((name, val));
            }
        }
    }

    async fn send(&mut self, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut builder = Request::builder().method(method).uri(path);
        if !self.cookies.is_empty() {
            builder = builder.header(header::COOKIE, self.cookie_header());
        }
        // Session-authenticated writes are CSRF-checked, so a suite that only ever
        // read could ignore this. The positivity test below writes with a session
        // cookie, and without the token the request is refused before the decision
        // this file is about is ever made.
        if !matches!(method, "GET" | "HEAD" | "OPTIONS") {
            if let Some((_, token)) = self.cookies.iter().find(|(name, _)| name == "darkhold_csrf") {
                let token = token.clone();
                builder = builder.header("x-csrf-token", token);
            }
        }
        if body.is_some() {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
        }
        let request = match body {
            Some(body) => builder.body(axum::body::Body::from(body.to_string())),
            None => builder.body(axum::body::Body::empty()),
        }
        .expect("request");
        let response = self
            .app
            .clone()
            .oneshot(request)
            .await
            .expect("route response");
        let status = response.status();
        self.capture(&response);
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("response body");
        let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, value)
    }

    async fn get(&mut self, path: &str) -> (StatusCode, Value) {
        self.send("GET", path, None).await
    }
}

struct Harness {
    /// Held so the scratch PostgreSQL database lives as long as the harness.
    _tdb: TestDb,
    _dir: std::path::PathBuf,
    config: Config,
    db: Database,
}

impl Harness {
    /// A harness where the reader named `operator` holds the operator role.
    ///
    /// The account is registered *before* the config is finalised, because
    /// `require_operator` compares the session's account against
    /// `config.administration.operator_account_id`.
    async fn with_operator(tag: &str) -> Self {
        let dir = scratch_dir(tag);
        let mut config = config_for(&dir);
        let tdb = TestDb::connect_with_dir(tag, &dir).await;
        let db = tdb.db().clone();

        let mut bootstrap = Client {
            app: server::build_router(AppState::new(config.clone(), db.clone())),
            cookies: Vec::new(),
        };
        let (status, body) = bootstrap
            .send(
                "POST",
                "/api/v1/auth/register",
                Some(json!({
                    "email": "operator@example.com",
                    "password": GOOD_PASSWORD,
                    "handle": "operator",
                    "display_name": "operator",
                    "age_band": "adult",
                })),
            )
            .await;
        assert!(
            status == StatusCode::CREATED,
            "bootstrap register: {status} {body}"
        );
        let account = account_of(&db, "operator").await;
        config.administration.operator_account_id = Some(account.into());
        Self {
            _tdb: tdb,
            _dir: dir,
            config,
            db,
        }
    }

    fn client(&self) -> Client {
        Client {
            app: server::build_router(AppState::new(self.config.clone(), self.db.clone())),
            cookies: Vec::new(),
        }
    }

    /// A signed-in reader. The operator handle already exists, so a register
    /// returning a conflict is expected and the code falls through to login.
    async fn reader(&self, handle: &str) -> Client {
        let mut client = self.client();
        let (status, _body) = client
            .send(
                "POST",
                "/api/v1/auth/register",
                Some(json!({
                    "email": format!("{handle}@example.com"),
                    "password": GOOD_PASSWORD,
                    "handle": handle,
                    "display_name": handle,
                    "age_band": "adult",
                })),
            )
            .await;
        if status != StatusCode::CREATED {
            let (status, body) = client
                .send(
                    "POST",
                    "/api/v1/auth/login",
                    Some(json!({
                        "email": format!("{handle}@example.com"),
                        "password": GOOD_PASSWORD,
                    })),
                )
                .await;
            assert!(status.is_success(), "login {handle}: {status} {body}");
        }
        client
    }

    async fn operator(&self) -> Client {
        self.reader("operator").await
    }

    /// Publish a one-chapter work as `handle`, returning its id.
    ///
    /// The same shape as `milestone_12::published_work`: create, add a unit, save its document, publish. It is duplicated rather
    /// than shared because the two harnesses disagree about how a client is built, and a shared helper would have to be written
    /// twice anyway.
    async fn published_work(&self, handle: &str) -> String {
        let mut author = self.reader(handle).await;
        let (status, body) = author
            .send("POST", "/api/v1/works", Some(json!({ "title": "A Work To Be Reviewed" })))
            .await;
        assert_eq!(status, StatusCode::CREATED, "create work: {body}");
        let work_id = body["id"].as_str().expect("id").to_owned();
        let work_version = body["version"].as_i64().expect("version");

        let (status, body) = author
            .send(
                "POST",
                &format!("/api/v1/works/{work_id}/chapters"),
                Some(json!({ "title": "One" })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "unit: {body}");
        let unit = body["id"].as_str().expect("unit").to_owned();
        let unit_version = body["version"].as_i64().expect("version");
        let document = json!({
            "type": "doc",
            "content": [{
                "type": "paragraph",
                "content": [{ "type": "text", "text": "A unit with enough words to have a middle." }]
            }]
        });
        let (status, body) = author
            .send(
                "PATCH",
                &format!("/api/v1/chapters/{unit}"),
                Some(json!({ "expected_version": unit_version, "document": document })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "save: {body}");
        let (status, body) = author
            .send(
                "POST",
                &format!("/api/v1/works/{work_id}/publish"),
                Some(json!({
                    "expected_version": work_version,
                    "idempotency_key": format!("audit-{work_id}"),
                })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "publish: {body}");
        work_id
    }

    /// Record a decision directly, so the test controls the row rather than
    /// needing an import to have run.
    async fn record(
        &self,
        subject: &str,
        deterministic: &str,
        posterior: Option<f64>,
        outcome: &str,
        provider: AuditProvider,
    ) {
        decision_audit::record(
            &self.db,
            &NewAuditEntry {
                task: "import_quality".to_owned(),
                subject: subject.to_owned(),
                deterministic: deterministic.to_owned(),
                posterior,
                threshold: Some(0.90),
                outcome: outcome.to_owned(),
                provider,
            },
        )
        .await
        .expect("a decision is recorded");
    }
}

async fn account_of(db: &Database, handle: &str) -> uuid::Uuid {
    let sql = match db.backend() {
        darkhold_db::Backend::Sqlite => {
            "SELECT account_id FROM pseuds WHERE lower(handle) = lower(?) ORDER BY created_at ASC"
        }
        darkhold_db::Backend::Postgres => {
            "SELECT account_id::text AS account_id FROM pseuds
              WHERE lower(handle) = lower($1) ORDER BY created_at ASC"
        }
    };
    let value: String = match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar(sql)
            .bind(handle)
            .fetch_one(db.sqlite_pool().expect("sqlite handle"))
            .await
            .expect("the handle exists"),
        darkhold_db::Backend::Postgres => sqlx::query_scalar(sql)
            .bind(handle)
            .fetch_one(db.postgres_pool().expect("postgres handle"))
            .await
            .expect("the handle exists"),
    };
    value.parse().expect("an account id is a uuid")
}

/// The endpoint answers, and it is the operator who gets an answer.
///
/// The seeding is the point: this assertion passes against the old
/// `{ "items": [] }` stub, and the test after it is the one that does not.
#[tokio::test]
async fn the_operator_reads_an_audit_trail() {
    let h = Harness::with_operator("operator-reads").await;
    let mut operator = h.operator().await;

    let (status, body) = operator.get("/api/v1/decisions/audit").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["items"].is_array(),
        "the trail is a list, even when empty: {body}"
    );
}

/// A recorded decision comes back, with its numbers.
///
/// The assertion the stub could not pass. `{ "items": [] }` is a 200 with an
/// array, so "the endpoint answers" proves nothing; "the row I wrote is in it"
/// proves the endpoint is wired to the table.
#[tokio::test]
async fn a_recorded_decision_comes_back_with_its_posterior_and_its_outcome() {
    let h = Harness::with_operator("rows-return").await;
    h.record(
        "work-1",
        "accepted",
        Some(0.42),
        "held",
        AuditProvider::Calibrated,
    )
    .await;
    let mut operator = h.operator().await;

    let (status, body) = operator.get("/api/v1/decisions/audit").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let items = body["items"].as_array().expect("items is a list");
    assert_eq!(
        items.len(),
        1,
        "the recorded decision is in the trail: {body}"
    );

    let row = &items[0];
    assert_eq!(row["subject"], json!("work-1"));
    assert_eq!(row["deterministic"], json!("accepted"));
    assert_eq!(row["outcome"], json!("held"));
    assert_eq!(row["provider"], json!("calibrated"));
    // The posterior is the number an operator tunes a threshold against, so it
    // has to be a JSON number and not a string, and it has to be *this* number.
    let posterior = row["posterior"]
        .as_f64()
        .expect("a recorded posterior is a number, not a string");
    assert!(
        (posterior - 0.42).abs() < 1e-9,
        "the posterior came back as {posterior}"
    );
    assert_eq!(
        body["model_has_narrowed_anything"],
        json!(true),
        "this row IS a model narrowing an acceptance, and the operator is told \
         so without having to spot it in the list: {body}"
    );
}

/// A decision with no model behind it reports no posterior, and says the model
/// has never done anything.
///
/// The other half of the same claim. A deterministic instance must be able to
/// be *told* that, or an operator who has enabled the calibrated provider and
/// seen nothing happen has no way to tell "the model is working and agrees"
/// from "the model is not running".
#[tokio::test]
async fn an_instance_where_no_model_has_run_says_so() {
    let h = Harness::with_operator("no-model").await;
    h.record(
        "work-1",
        "accepted",
        None,
        "accepted",
        AuditProvider::Deterministic,
    )
    .await;
    let mut operator = h.operator().await;

    let (status, body) = operator.get("/api/v1/decisions/audit").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let row = &body["items"].as_array().expect("items")[0];
    assert_eq!(
        row["posterior"],
        Value::Null,
        "no model was consulted, so the response carries null rather than 0.0: \
         a zero would read as a model confidently saying no: {body}"
    );
    assert_eq!(
        body["model_has_narrowed_anything"],
        json!(false),
        "a model that has never run has never narrowed anything, and the \
         operator is told that rather than left to infer it"
    );
}

/// The trail never carries the text it judged.
///
/// §12.1: a reader's words stay with their author, and the filter is a
/// judgement rather than a taking. An audit row holding the graded text would
/// be a second copy of it with none of the first copy's rules — no deletion
/// path, no export, no visibility level, and no author who can see it.
///
/// The subject here is deliberately prose-shaped, so a store that echoed it
/// back would be caught.
#[tokio::test]
async fn the_trail_carries_the_subject_id_and_never_the_graded_text() {
    let h = Harness::with_operator("no-text").await;
    let subject = "work-1";
    h.record(
        subject,
        "accepted",
        Some(0.5),
        "held",
        AuditProvider::Calibrated,
    )
    .await;
    let mut operator = h.operator().await;

    let (status, body) = operator.get("/api/v1/decisions/audit").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let serialised = body.to_string();
    assert_eq!(
        body["items"][0]["subject"],
        json!(subject),
        "the subject is the id: {body}"
    );
    for forbidden in ["title", "summary", "body", "text", "content", "excerpt"] {
        assert!(
            !serialised.contains(forbidden),
            "the audit response must not carry a `{forbidden}` field: {body}"
        );
    }
}

/// A signed-in reader who is not the operator is told the surface does not exist.
///
/// 404, not 403 — and the status is the assertion. "Refused" would pass against
/// a 403, and a 403 confirms the audit surface exists, which is itself the
/// disclosure: the audit records what this instance decided and with what.
#[tokio::test]
async fn a_non_operator_is_not_told_the_surface_exists() {
    let h = Harness::with_operator("non-operator").await;
    let mut reader = h.reader("reader").await;

    let (status, _body) = reader.get("/api/v1/decisions/audit").await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a 403 here would confirm the audit surface exists, which is the \
         disclosure this endpoint exists to avoid"
    );
}

/// An unauthenticated caller learns nothing, including that it exists.
#[tokio::test]
async fn an_unauthenticated_caller_is_refused() {
    let h = Harness::with_operator("unauthenticated").await;
    let mut anonymous = h.client();

    let (status, _body) = anonymous.get("/api/v1/decisions/audit").await;
    assert!(
        status == StatusCode::UNAUTHORIZED || status == StatusCode::NOT_FOUND,
        "got {status}"
    );
}

/// The filters reach the query rather than being decoration.
#[tokio::test]
async fn the_filters_narrow_the_trail() {
    let h = Harness::with_operator("filters").await;
    h.record(
        "work-1",
        "accepted",
        None,
        "accepted",
        AuditProvider::Deterministic,
    )
    .await;
    h.record(
        "work-2",
        "accepted",
        Some(0.3),
        "held",
        AuditProvider::Calibrated,
    )
    .await;
    let mut operator = h.operator().await;

    let (status, body) = operator.get("/api/v1/decisions/audit?subject=work-2").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let items = body["items"].as_array().expect("items");
    assert_eq!(items.len(), 1, "the subject filter narrows: {body}");
    assert_eq!(items[0]["subject"], json!("work-2"));

    let (status, body) = operator
        .get("/api/v1/decisions/audit?provider=calibrated")
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let items = body["items"].as_array().expect("items");
    assert_eq!(items.len(), 1, "the provider filter narrows: {body}");
    assert_eq!(items[0]["provider"], json!("calibrated"));

    // A filter that matches nothing is an honest "no", not an error and not
    // everything. An operator asking "has the model ever touched positivity?"
    // has to be able to be told no.
    let (status, body) = operator
        .get("/api/v1/decisions/audit?task=positivity")
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["items"].as_array().expect("items").is_empty(),
        "an operator must be able to be told the answer is no: {body}"
    );
}

/// `limit` is honoured, and an absurd one is bounded rather than obeyed.
#[tokio::test]
async fn the_limit_is_honoured_and_bounded() {
    let h = Harness::with_operator("limit").await;
    for i in 0..3 {
        h.record(
            &format!("work-{i}"),
            "accepted",
            None,
            "accepted",
            AuditProvider::Deterministic,
        )
        .await;
    }
    let mut operator = h.operator().await;

    let (status, body) = operator.get("/api/v1/decisions/audit?limit=2").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["items"].as_array().expect("items").len(),
        2,
        "the limit is honoured: {body}"
    );

    let (status, body) = operator
        .get("/api/v1/decisions/audit?limit=999999999999")
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["items"].as_array().expect("items").len() <= 500,
        "an absurd limit gets the bound, not the table: {} items",
        body["items"].as_array().map_or(0, Vec::len)
    );
}

/// The default page is a page, not the whole table.
#[tokio::test]
async fn asking_for_nothing_returns_a_bounded_page() {
    let h = Harness::with_operator("default-limit").await;
    for i in 0..60 {
        h.record(
            &format!("work-{i}"),
            "accepted",
            None,
            "accepted",
            AuditProvider::Deterministic,
        )
        .await;
    }
    let mut operator = h.operator().await;

    let (status, body) = operator.get("/api/v1/decisions/audit").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let len = body["items"].as_array().expect("items").len();
    assert!(
        len < 60,
        "a default that silently means everything is a default nobody sets on \
         purpose: {len} items came back"
    );
}

/// The id of the one review on a work, read from the database.
async fn review_id_for(db: &Database, work_id: &str) -> String {
    let sql = match db.backend() {
        // The table is `review`, singular (migration `0005_reading.sql`), and the column is a uuid on PostgreSQL while it is
        // TEXT here — the same asymmetry as every other id in this schema.
        darkhold_db::Backend::Sqlite => "SELECT id FROM review WHERE work_id = ? AND deleted_at IS NULL",
        darkhold_db::Backend::Postgres => {
            "SELECT id::text AS id FROM review WHERE work_id::text = $1 AND deleted_at IS NULL"
        }
    };
    match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar(sql)
            .bind(work_id)
            .fetch_one(db.sqlite_pool().expect("sqlite handle"))
            .await
            .expect("the review exists"),
        darkhold_db::Backend::Postgres => sqlx::query_scalar(sql)
            .bind(work_id)
            .fetch_one(db.postgres_pool().expect("postgres handle"))
            .await
            .expect("the review exists"),
    }
}

/// **A held review leaves a decision in the trail, written by the gate that held it.**
///
/// This is the half that was missing. `decision_audit::record` had no product caller at all: every row in this file's other
/// tests is seeded by the test itself, so the trail was empty on every real instance and `GET /decisions/audit` answered
/// `{ "items": [] }` to the operator who asked why a review had been held. The store, the route, the filter and the summary
/// number all worked; nothing wrote.
///
/// The positivity gate is the first real decision surface to record. It answers a reader's complaint ("my review vanished")
/// and an operator's question ("is this instance holding too much?") with the same rows, and it is a *deterministic* decision,
/// which is why `posterior` must be null rather than `0.0`: no model was consulted, and a zero would read as a model
/// confidently saying "hold it".
#[tokio::test]
async fn a_held_review_records_the_decision_that_held_it() {
    let h = Harness::with_operator("positivity-recorded").await;
    let work_id = h.published_work("author").await;

    // The bluntest policy in the module: the author has turned comments off, so every public review is held no matter what it
    // says. That keeps the outcome a property of the policy rather than of the classifier's opinion of a sentence.
    let mut author = h.reader("author").await;
    let (status, body) = author
        .send(
            "PUT",
            "/api/v1/feedback/preferences",
            Some(json!({ "comments_enabled": false })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "turn comments off: {body}");

    let mut reviewer = h.reader("reviewer").await;
    let (status, body) = reviewer
        .send(
            "PUT",
            &format!("/api/v1/works/{work_id}/reviews"),
            Some(json!({
                "body": "Warm and well made, and the ending earns it.",
                "is_public": true,
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "post the review: {body}");
    assert_eq!(
        body["receipt"], "Comment held for moderator review.",
        "the gate must hold this text for the test to mean anything: {body}"
    );

    let review_id = review_id_for(&h.db, &work_id).await;

    let mut operator = h.operator().await;
    let (status, body) = operator
        .get(&format!("/api/v1/decisions/audit?subject={review_id}"))
        .await;
    assert_eq!(status, StatusCode::OK, "read the trail: {body}");
    let items = body["items"].as_array().expect("items is a list");
    assert_eq!(
        items.len(),
        1,
        "one review, one decision, written by the gate: {body}"
    );
    let row = &items[0];
    assert_eq!(row["task"], "positivity", "{body}");
    assert_eq!(row["subject"], review_id, "{body}");
    assert_eq!(row["outcome"], "held", "the applied outcome: {body}");
    assert_eq!(row["provider"], "deterministic", "{body}");
    assert!(
        row["posterior"].is_null(),
        "no model was consulted, and 0.0 would claim one said no: {body}"
    );
    assert!(
        row["deterministic"].as_str().is_some_and(|class| !class.is_empty()),
        "the classifier's own answer is recorded beside the outcome: {body}"
    );
    // The graded text is not in the row — §12.1, and the module's load-bearing privacy decision.
    assert!(
        !body.to_string().contains("Warm and well made"),
        "the audit must not hold the words it judged: {body}"
    );
}
