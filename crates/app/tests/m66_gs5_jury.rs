//! GS-5 — the jury, at the doors.
//!
//! `crates/domain/src/jury.rs` proves the rule: same seed, same panel; exclusions structural; elastic fallback; refuse a pool that is too small. This
//! file proves the seam — that the rule is reached by a real request, that what it drew is written down and re-readable, that an appeal excludes the
//! people who already decided, and that a juror votes once.
//!
//! The five cases, each a question the store or the route could get wrong while the rule stayed green:
//!
//! 1. **The parties never sit.** A pool of ten, a report by one of them against another — the panel of five contains neither, and the account that
//!    is not on the panel cannot read it.
//! 2. **A draw happens once.** A second `POST` answers with the panel that exists rather than drawing a new one, because the stored seed is the audit
//!    handle and redrawing would make it a lie.
//! 3. **An appeal is a fresh jury.** `appeal: true` draws from the pool minus the original members, and records which panel it replaced.
//! 4. **A juror votes once, and sees the split only after voting.** The second vote is refused; the tally is visible to a juror who has answered.
//! 5. **A pool below the elastic floor is refused, and nothing is written.** The refusal names the numbers, and a later draw with a bigger pool is
//!    possible — which is only true because the failed attempt left no row behind.

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server::{self, set_trust_proxy};
use darkhold_app::state::AppState;
use darkhold_db::{Backend, DatabaseConfig};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tower::ServiceExt;

const PASSWORD: &str = "test-pass-1234";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-gs5-{tag}-{}-{:?}",
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
    config.database = match std::env::var("DARKHOLD_TEST_PG_URL") {
        Ok(url) if !url.is_empty() => DatabaseConfig::new(url),
        _ => DatabaseConfig::new(format!("sqlite://{}/darkhold.sqlite?mode=rwc", dir.display())),
    };
    // This suite registers a dozen-plus accounts per case, which is the point — a jury needs a pool — and the dev defaults' auth burst is built for a
    // person, not a fixture. Raised here rather than in the shipped default, because a real instance's sign-up burst should stay small.
    config.rate_limits.auth = darkhold_app::limiter::Quota {
        burst: 500,
        per_minute: 5_000,
    };
    config.rate_limits.write = darkhold_app::limiter::Quota {
        burst: 500,
        per_minute: 5_000,
    };
    config
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
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json");
        if !self.cookies.is_empty() {
            let joined = self
                .cookies
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("; ");
            builder = builder.header(header::COOKIE, joined);
        }
        if let Some(token) = self
            .cookies
            .iter()
            .find(|(k, _)| k == "darkhold_csrf")
            .map(|(_, v)| v.to_owned())
        {
            builder = builder.header("x-csrf-token", token);
        }
        let body = match body {
            Some(value) => Body::from(value.to_string()),
            None => Body::empty(),
        };
        let response = self
            .app
            .clone()
            .oneshot(builder.body(body).unwrap())
            .await
            .unwrap();
        let status = response.status();
        self.capture(&response);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn post(&mut self, uri: &str, body: Value) -> (StatusCode, Value) {
        self.send("POST", uri, Some(body)).await
    }

    async fn get(&mut self, uri: &str) -> (StatusCode, Value) {
        self.send("GET", uri, None).await
    }
}

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

    fn client(&self) -> Client {
        Client::new(server::build_router(AppState::new(
            self.config.clone(),
            self.tdb.db().clone(),
        )))
    }

    /// A registered account at a chosen trust level. `level` is written straight to `trust_levels`, which is what the pool reads: the draw must use
    /// the stored level, not one recomputed at draw time.
    async fn account(&self, handle: &str, level: i64) -> (Client, String) {
        let mut client = self.client();
        let (status, registered) = client
            .post(
                "/api/v1/auth/register",
                json!({
                    "email": format!("{handle}@example.test"),
                    "password": PASSWORD,
                    "handle": handle,
                    "display_name": handle,
                    "age_band": "adult",
                }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{handle}: {registered}");
        let account = registered["account"]["id"]
            .as_str()
            .expect("account id")
            .to_owned();
        darkhold_db::governance::set_trust(self.tdb.db(), &account, level, "test")
            .await
            .expect("set trust");
        (client, account)
    }

    /// A report by `reporter` against `reported`, and the review task it opened. Written directly because the route that files a report also
    /// assigns it, and the task id is what the jury hangs from.
    async fn report_task(&self, reporter: &str, reported: &str) -> String {
        let report = uuid::Uuid::new_v4().to_string();
        let task = uuid::Uuid::new_v4().to_string();
        let at = "2026-01-01T00:00:00Z";
        match self.tdb.db().backend() {
            Backend::Sqlite => {
                sqlx::query(
                    "INSERT INTO reports (id, subject_type, subject_id, reporter, reason, created_at, state, resolution)
                     VALUES (?1, 'account', ?2, ?3, 'spam', ?4, 'open', '')",
                )
                .bind(&report)
                .bind(reported)
                .bind(reporter)
                .bind(at)
                .execute(self.tdb.db().sqlite_pool().expect("sqlite"))
                .await
                .expect("report");
                sqlx::query(
                    "INSERT INTO review_tasks (id, reviewer, report_id, assigned_at, outcome) VALUES (?1, '', ?2, ?3, '')",
                )
                .bind(&task)
                .bind(&report)
                .bind(at)
                .execute(self.tdb.db().sqlite_pool().expect("sqlite"))
                .await
                .expect("task");
            }
            Backend::Postgres => {
                sqlx::query(
                    "INSERT INTO reports (id, subject_type, subject_id, reporter, reason, created_at, state, resolution)
                     VALUES ($1, 'account', $2, $3, 'spam', $4, 'open', '')",
                )
                .bind(&report)
                .bind(reported)
                .bind(reporter)
                .bind(at)
                .execute(self.tdb.db().postgres_pool().expect("postgres"))
                .await
                .expect("report");
                sqlx::query(
                    "INSERT INTO review_tasks (id, reviewer, report_id, assigned_at, outcome) VALUES ($1, '', $2, $3, '')",
                )
                .bind(&task)
                .bind(&report)
                .bind(at)
                .execute(self.tdb.db().postgres_pool().expect("postgres"))
                .await
                .expect("task");
            }
        }
        task
    }

    async fn panel_rows(&self, task: &str) -> i64 {
        match self.tdb.db().backend() {
            Backend::Sqlite => sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM jury_panels WHERE task_id = ?1",
            )
            .bind(task)
            .fetch_one(self.tdb.db().sqlite_pool().expect("sqlite"))
            .await
            .expect("count"),
            Backend::Postgres => sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM jury_panels WHERE task_id = $1",
            )
            .bind(task)
            .fetch_one(self.tdb.db().postgres_pool().expect("postgres"))
            .await
            .expect("count"),
        }
    }
}

fn seated(response: &Value) -> Vec<String> {
    response["members"]
        .as_array()
        .expect("members")
        .iter()
        .map(|member| member["account"].as_str().expect("account").to_owned())
        .collect()
}

#[tokio::test]
async fn a_panel_is_drawn_and_the_parties_never_sit() {
    let harness = Harness::new("draw").await;
    let (mut service, _) = harness.account("service", 6).await;

    let mut pool = Vec::new();
    for index in 0..10 {
        let (_, account) = harness.account(&format!("juror{index}"), 3).await;
        pool.push(account);
    }
    let (_, reporter) = harness.account("reporter", 3).await;
    let (_, reported) = harness.account("reported", 3).await;
    let task = harness.report_task(&reporter, &reported).await;

    let (status, body) = service
        .post(
            &format!("/api/v1/reports/{task}/jury"),
            json!({ "seed": "audit-1" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "draw: {status} {body}");
    assert_eq!(body["size"], json!(5), "{body}");
    assert_eq!(body["seed"], json!("audit-1"), "the stored seed is echoed");

    let panel = seated(&body);
    assert_eq!(panel.len(), 5, "a full panel: {body}");
    assert!(
        !panel.contains(&reporter),
        "the reporter must not sit: {panel:?}"
    );
    assert!(
        !panel.contains(&reported),
        "the reported must not sit: {panel:?}"
    );
    // Drawn from the pool of ten, not from the three: every seat is an eligible bystander.
    assert!(
        panel.iter().all(|account| pool.contains(account)),
        "every seat is drawn from the eligible pool: {panel:?}"
    );

    // A stranger — here an anonymous client — cannot read the panel. 404, not 403: confirming that a panel exists is a fact about a moderation
    // decision on somebody else.
    let mut anonymous = harness.client();
    let (status, refused) = anonymous.get(&format!("/api/v1/reports/{task}/jury")).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "the door is behind a session at all: {status} {refused}"
    );

    // A signed-in reader who is neither seated nor a service gets 404 from the handler: "you are not on this jury" is a fact about somebody's
    // moderation decision, and a 403 would confirm the panel exists.
    let (mut bystander, bystander_account) = harness.account("bystander", 3).await;
    assert!(!panel.contains(&bystander_account));
    let (status, refused) = bystander.get(&format!("/api/v1/reports/{task}/jury")).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a bystander must not read the panel: {status} {refused}"
    );
}

#[tokio::test]
async fn a_second_draw_is_the_panel_that_exists() {
    let harness = Harness::new("once").await;
    let (mut service, _) = harness.account("service", 6).await;
    for index in 0..8 {
        harness.account(&format!("seat{index}"), 3).await;
    }
    let (_, reporter) = harness.account("reporter", 3).await;
    let (_, reported) = harness.account("reported", 3).await;
    let task = harness.report_task(&reporter, &reported).await;

    let (status, first) = service
        .post(
            &format!("/api/v1/reports/{task}/jury"),
            json!({ "seed": "audit-2" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{first}");

    let (status, second) = service
        .post(
            &format!("/api/v1/reports/{task}/jury"),
            json!({ "seed": "a-different-seed" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(
        second["panel"], first["panel"],
        "a second draw answers with the panel that exists: {first} then {second}"
    );
    assert_eq!(
        harness.panel_rows(&task).await,
        1,
        "and it wrote nothing new"
    );
}

#[tokio::test]
async fn an_appeal_draws_from_the_pool_without_the_original_members() {
    let harness = Harness::new("appeal").await;
    let (mut service, _) = harness.account("service", 6).await;
    for index in 0..12 {
        harness.account(&format!("seat{index}"), 3).await;
    }
    let (_, reporter) = harness.account("reporter", 3).await;
    let (_, reported) = harness.account("reported", 3).await;
    let task = harness.report_task(&reporter, &reported).await;

    let (_, first) = service
        .post(
            &format!("/api/v1/reports/{task}/jury"),
            json!({ "seed": "audit-3" }),
        )
        .await;
    let original = seated(&first);
    assert_eq!(original.len(), 5, "{first}");

    let (status, appeal) = service
        .post(
            &format!("/api/v1/reports/{task}/jury"),
            json!({ "seed": "audit-4", "appeal": true }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "appeal: {status} {appeal}");

    let fresh = seated(&appeal);
    assert_eq!(fresh.len(), 5, "{appeal}");
    assert_eq!(
        appeal["appeal_of"], first["panel"],
        "the appeal records which panel it replaced: {appeal}"
    );
    for member in &fresh {
        assert!(
            !original.contains(member),
            "an appeal must not reuse a member of the panel it appeals: {member} sat on {original:?}"
        );
    }
}

#[tokio::test]
async fn a_juror_votes_once_and_sees_the_split_only_after_voting() {
    let harness = Harness::new("vote").await;
    let (mut service, _) = harness.account("service", 6).await;
    let mut jurors = Vec::new();
    for index in 0..8 {
        let (client, account) = harness.account(&format!("seat{index}"), 3).await;
        jurors.push((client, account));
    }
    let (_, reporter) = harness.account("reporter", 3).await;
    let (_, reported) = harness.account("reported", 3).await;
    let task = harness.report_task(&reporter, &reported).await;

    let (_, drawn) = service
        .post(
            &format!("/api/v1/reports/{task}/jury"),
            json!({ "seed": "audit-5" }),
        )
        .await;
    let panel = seated(&drawn);
    let (mut client, account) = jurors
        .into_iter()
        .find(|(_, account)| panel.contains(account))
        .expect("a seated juror with their client");

    let (status, before) = client.get(&format!("/api/v1/reports/{task}/jury")).await;
    assert_eq!(status, StatusCode::OK, "{before}");
    assert_eq!(
        before["votes_shown"],
        json!(false),
        "a juror who has not voted does not see the split: {before}"
    );
    assert_eq!(before["open"], json!(true), "{before}");

    let (status, voted) = client
        .post(
            &format!("/api/v1/reports/{task}/jury/verdict"),
            json!({ "verdict": "overturn" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "vote: {status} {voted}");
    assert_eq!(voted["tally"]["overturn"], json!(1), "{voted}");
    assert_eq!(
        voted["votes_shown"],
        json!(true),
        "after voting the juror may see the split: {voted}"
    );

    let (status, again) = client
        .post(
            &format!("/api/v1/reports/{task}/jury/verdict"),
            json!({ "verdict": "uphold" }),
        )
        .await;
    assert!(
        status == StatusCode::UNPROCESSABLE_ENTITY || status == StatusCode::CONFLICT,
        "a jury is not a poll: {status} {again}"
    );
    let (_, after) = client.get(&format!("/api/v1/reports/{task}/jury")).await;
    assert_eq!(
        after["tally"]["overturn"],
        json!(1),
        "the second vote changed nothing: {after}"
    );
    assert_eq!(after["tally"]["uphold"], json!(0), "{after}");
    let _ = account;
}

#[tokio::test]
async fn a_pool_below_the_elastic_floor_is_refused_and_writes_nothing() {
    let harness = Harness::new("small").await;
    let (mut service, _) = harness.account("service", 6).await;
    // Two bystanders and the two parties: five accounts, two of them excluded, so two eligible — below `jury_elastic_min` (3).
    harness.account("seat0", 1).await;
    harness.account("seat1", 1).await;
    let (_, reporter) = harness.account("reporter", 1).await;
    let (_, reported) = harness.account("reported", 1).await;
    let task = harness.report_task(&reporter, &reported).await;

    let (status, body) = service
        .post(
            &format!("/api/v1/reports/{task}/jury"),
            json!({ "seed": "audit-6" }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a two-person pool decides nothing: {status} {body}"
    );
    // The numbers, not just "no": the operator who reads this has to know whether to invite three people or thirty.
    let rendered = body.to_string();
    assert!(
        rendered.contains("at least 3 eligible accounts") && rendered.contains("has 2"),
        "the refusal names what it needed and what it found: {body}"
    );
    assert_eq!(
        harness.panel_rows(&task).await,
        0,
        "a refused draw must leave no row behind, or the report can never get a panel: {body}"
    );
}

/// The panel's decision reaches the report. A jury that votes into a void has not decided anything.
///
/// Every juror votes `overturn`, so the majority is not ambiguous; the assertion is that the *task* and the *report* changed, not just the member rows
/// — which is the failure this test exists to catch, because the verdict surface looked finished while the queue never moved.
#[tokio::test]
async fn the_last_verdict_is_the_decision() {
    let harness = Harness::new("decide").await;
    let (mut service, _) = harness.account("service", 6).await;
    let mut jurors = Vec::new();
    for index in 0..8 {
        let (client, account) = harness.account(&format!("seat{index}"), 3).await;
        jurors.push((client, account));
    }
    let (_, reporter) = harness.account("reporter", 3).await;
    let (_, reported) = harness.account("reported", 3).await;
    let task = harness.report_task(&reporter, &reported).await;

    let (_, drawn) = service
        .post(
            &format!("/api/v1/reports/{task}/jury"),
            json!({ "seed": "audit-7" }),
        )
        .await;
    let panel = seated(&drawn);

    let mut last = Value::Null;
    for (mut client, account) in jurors.into_iter() {
        if !panel.contains(&account) {
            continue;
        }
        let (status, body) = client
            .post(
                &format!("/api/v1/reports/{task}/jury/verdict"),
                json!({ "verdict": "overturn" }),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "vote: {status} {body}");
        last = body;
    }

    assert_eq!(last["closed"], json!(true), "every seat answered: {last}");
    assert_eq!(last["tally"]["overturn"], json!(5), "{last}");
    assert_eq!(
        last["decided"],
        json!("overturn"),
        "the last verdict is the decision: {last}"
    );

    // And the queue moved: the task is decided and the report is resolved with the same word.
    let (outcome, state, resolution) = match harness.tdb.db().backend() {
        Backend::Sqlite => {
            let row: (String, String, String) = sqlx::query_as(
                "SELECT t.outcome, r.state, r.resolution FROM review_tasks t
                   JOIN reports r ON r.id = t.report_id WHERE t.id = ?1",
            )
            .bind(&task)
            .fetch_one(harness.tdb.db().sqlite_pool().expect("sqlite"))
            .await
            .expect("task and report");
            row
        }
        Backend::Postgres => {
            let row: (String, String, String) = sqlx::query_as(
                "SELECT t.outcome, r.state, r.resolution FROM review_tasks t
                   JOIN reports r ON r.id = t.report_id WHERE t.id = $1",
            )
            .bind(&task)
            .fetch_one(harness.tdb.db().postgres_pool().expect("postgres"))
            .await
            .expect("task and report");
            row
        }
    };
    assert_eq!(outcome, "overturn", "the task carries the jury's verdict");
    assert_eq!(state, "resolved", "the report is closed by the decision");
    assert_eq!(resolution, "overturn", "with the same word, not a synonym");
}
