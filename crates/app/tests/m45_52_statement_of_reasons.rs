//! M45-52 steps 1–2 — a decision carries its reasons.
//!
//! The measured starting point: `decide_task` stored an outcome string and an audit entry. The notifier got an answer;
//! the author whose work was acted on got a sanction and no explanation; and nothing recorded *why* in a form an appeal
//! or a regulator could read.
//!
//! What these tests pin, and why each one is a refusal rather than a feature:
//!
//! 1. **A decision without a ground is refused `422`, naming `ground`.** If the statement is optional the queue fills
//!    with unexplained decisions within a week — the reviewer is the only person who knows why, and they are about to
//!    move on to the next report.
//! 2. **The statement written with the decision names the reviewer and the deadline.** A reason with no author is an
//!    opinion, and a deadline that is computed at appeal time instead of stored can close a window somebody was told
//!    about.
//! 3. **The two lawful grounds refuse to omit the instrument.** `illegal_content` and `copyright` are claims about law;
//!    without the statute they are unfalsifiable, and an appeal cannot test them.

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server::{self, set_trust_proxy};
use darkhold_app::state::AppState;
use darkhold_db::DatabaseConfig;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tower::ServiceExt;

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-m45-52-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn config_for(dir: &Path) -> Config {
    let mut config = Config::development_defaults();
    // The rate-limit buckets are process-global at 127.0.0.1, so with nine tests registering and signing in at once the development
    // defaults are exhausted by neighbouring cases and a register answers 429 — which reads as a broken suite rather than a busy one.
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
    config.storage.root = dir.to_path_buf();
    config.database = match std::env::var("DARKHOLD_TEST_PG_URL") {
        Ok(url) if !url.is_empty() => DatabaseConfig::new(url),
        _ => DatabaseConfig::new(format!(
            "sqlite://{}/darkhold.sqlite?mode=rwc",
            dir.display()
        )),
    };
    config
}

const PASSWORD: &str = "test-pass-1234";

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

    async fn request(
        &mut self,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json");
        for (name, value) in &self.cookies {
            builder = builder.header(header::COOKIE, format!("{name}={value}"));
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
            Some(v) => Body::from(v.to_string()),
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
        let json: Value = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
        (status, json)
    }

    async fn post(&mut self, uri: &str, body: Value) -> (StatusCode, Value) {
        self.request("POST", uri, Some(body)).await
    }

    async fn get(&mut self, uri: &str) -> (StatusCode, Value) {
        self.request("GET", uri, None).await
    }

    async fn register(&mut self, email: &str, handle: &str) -> String {
        let (status, body) = self
            .post(
                "/api/v1/auth/register",
                json!({
                    "email": email,
                    "password": PASSWORD,
                    "handle": handle,
                    "display_name": handle,
                    "age_band": "adult",
                }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "register {handle}: {body}");
        let (status, me) = self.get("/api/v1/auth/me").await;
        assert_eq!(status, StatusCode::OK, "{me}");
        me["account"]["id"].as_str().expect("account id").to_owned()
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

    fn db(&self) -> &darkhold_db::Database {
        self.tdb.db()
    }

    fn client(&self) -> Client {
        Client::new(server::build_router(AppState::new(
            self.config.clone(),
            self.tdb.db().clone(),
        )))
    }

    /// Run one statement against whichever engine the harness is on.
    async fn exec(&self, statement: &str) {
        match self.db().backend() {
            darkhold_db::Backend::Sqlite => {
                sqlx::query(statement)
                    .execute(self.db().sqlite_pool().expect("sqlite"))
                    .await
                    .expect("seed statement");
            }
            darkhold_db::Backend::Postgres => {
                sqlx::query(statement)
                    .execute(self.db().postgres_pool().expect("postgres"))
                    .await
                    .expect("seed statement");
            }
        }
    }

    /// A reviewer with standing to resolve reports, the task a report became, and the reviewer's account id.
    ///
    /// One client does the whole thing — register, grant standing, assign, decide — because the point of the row is what
    /// *the act of deciding* must carry, not who assigned it.
    async fn assigned_task_with_target(&self, tag: &str) -> (Client, String, String, String) {
        let mut reviewer = self.client();
        let reviewer_account = reviewer
            .register(&format!("{tag}-reviewer@example.test"), "Reviewer")
            .await;
        darkhold_db::governance::set_trust(self.db(), &reviewer_account, 6, "test")
            .await
            .expect("grant reviewing standing");

        let target = {
            let mut target = self.client();
            target
                .register(&format!("{tag}-target@example.test"), "Target")
                .await
        };

        let mut reporter = self.client();
        reporter
            .register(&format!("{tag}-reporter@example.test"), "Reporter")
            .await;
        let (status, body) = reporter
            .post(
                "/api/v1/reports",
                json!({ "subject_type": "account", "subject_id": target, "reason": "spam" }),
            )
            .await;
        assert!(
            status == StatusCode::OK || status == StatusCode::CREATED,
            "file the report: {status} {body}"
        );
        let report_id = body["id"].as_str().expect("report id").to_owned();

        let (status, body) = reviewer
            .post(
                &format!("/api/v1/moderation/reports/{report_id}/assign"),
                json!({}),
            )
            .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "assign the review task: {status} {body}"
        );
        let task_id = body["task_id"].as_str().expect("task id").to_owned();
        (reviewer, task_id, reviewer_account, target)
    }
}

impl Harness {
    /// The three-value form the statement tests use.
    async fn assigned_task(&self, tag: &str) -> (Client, String, String) {
        let (reviewer, task_id, account, _target) = self.assigned_task_with_target(tag).await;
        (reviewer, task_id, account)
    }
}

#[tokio::test]
async fn a_decision_without_a_ground_is_refused_naming_the_field() {
    let harness = Harness::new("noground").await;
    let (mut reviewer, task_id, _account) = harness.assigned_task("noground").await;

    let (status, body) = reviewer
        .post(
            &format!("/api/v1/moderation/tasks/{task_id}/decide"),
            json!({ "outcome": "uphold" }),
        )
        .await;

    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a decision with no reasons is refused: {status} {body}"
    );
    let field_errors = &body["error"]["field_errors"];
    assert!(
        field_errors.get("ground").is_some(),
        "the refusal names the field that is missing: {body}"
    );
    assert!(
        darkhold_db::governance::statement_for_task(harness.db(), &task_id)
            .await
            .expect("read statements")
            .is_none(),
        "and nothing was written: a refused decision leaves no statement behind"
    );
}

#[tokio::test]
async fn the_statement_written_with_the_decision_names_the_reviewer_and_the_deadline() {
    let harness = Harness::new("statement").await;
    let (mut reviewer, task_id, reviewer_account) = harness.assigned_task("statement").await;

    let (status, body) = reviewer
        .post(
            &format!("/api/v1/moderation/tasks/{task_id}/decide"),
            json!({
                "outcome": "uphold",
                "ground": "spam",
                "facts": "Six identical promotional replies within one hour, from an account created that morning.",
            }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "uphold with reasons: {status} {body}"
    );
    let appeal_until = body["appeal_until"].as_str().expect("deadline").to_owned();
    assert_eq!(body["ground"], "spam");
    assert!(body["statement_id"]
        .as_str()
        .is_some_and(|id| !id.is_empty()));

    let statement = darkhold_db::governance::statement_for_task(harness.db(), &task_id)
        .await
        .expect("read statements")
        .expect("a statement was written with the decision");

    assert_eq!(statement.ground, "spam");
    assert_eq!(statement.outcome, "uphold");
    assert!(
        statement.facts.contains("identical promotional replies"),
        "the reviewer's own words are kept verbatim: {}",
        statement.facts
    );
    assert_eq!(
        statement.decided_by, reviewer_account,
        "a reason with no author is an opinion"
    );
    assert_eq!(
        statement.appeal_until, appeal_until,
        "one deadline, reported and stored"
    );
    assert!(
        statement.appeal_until > statement.decided_at,
        "the window opens at the decision: {} vs {}",
        statement.appeal_until,
        statement.decided_at
    );
}

#[tokio::test]
async fn a_lawful_ground_must_name_its_instrument() {
    let harness = Harness::new("legal").await;
    let (mut reviewer, task_id, _account) = harness.assigned_task("legal").await;

    let (status, body) = reviewer
        .post(
            &format!("/api/v1/moderation/tasks/{task_id}/decide"),
            json!({
                "outcome": "uphold",
                "ground": "copyright",
                "facts": "The text is a verbatim copy of a commercially published chapter.",
            }),
        )
        .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{status} {body}");
    assert!(
        body["error"]["field_errors"].get("legal_ground").is_some(),
        "copyright is a claim about law, so it must name the rule: {body}"
    );
}

/// Issuing the sanction the decision justifies, with the reasons filed under the statement.
async fn sanction_from_statement(
    reviewer: &mut Client,
    target: &str,
    statement_id: &str,
) -> String {
    let (status, body) = reviewer
        .post(
            "/api/v1/sanctions",
            json!({
                "account": target,
                "kind": "rate_limit",
                "reason_ref": format!("statement:{statement_id}"),
                "duration_days": 7,
            }),
        )
        .await;
    assert!(
        status == StatusCode::OK || status == StatusCode::CREATED,
        "issue the sanction: {status} {body}"
    );
    body["id"]
        .as_str()
        .or_else(|| body["sanction_id"].as_str())
        .expect("sanction id")
        .to_owned()
}

#[tokio::test]
async fn an_appeal_inside_the_window_is_accepted() {
    let harness = Harness::new("inside").await;
    let (mut reviewer, task_id, _account, target) =
        harness.assigned_task_with_target("inside").await;

    let (status, body) = reviewer
        .post(
            &format!("/api/v1/moderation/tasks/{task_id}/decide"),
            json!({
                "outcome": "uphold",
                "ground": "spam",
                "facts": "Six identical promotional replies within one hour.",
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{status} {body}");
    let statement_id = body["statement_id"]
        .as_str()
        .expect("statement id")
        .to_owned();
    let sanction = sanction_from_statement(&mut reviewer, &target, &statement_id).await;

    // The sanction is against `target`, so the appeal must come from that account. Sign in as it.
    let mut owner = harness.client();
    let (status, body) = owner
        .post(
            "/api/v1/auth/login",
            json!({ "email": "inside-target@example.test", "password": PASSWORD }),
        )
        .await;
    assert!(
        status == StatusCode::OK || status == StatusCode::CREATED,
        "sign in: {status} {body}"
    );

    let (status, body) = owner
        .post("/api/v1/appeals", json!({ "sanction_id": sanction, "statement": "The replies were answers to direct questions." }))
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "inside the window an appeal is accepted: {status} {body}"
    );
    assert!(body["id"].as_str().is_some_and(|id| !id.is_empty()));
}

#[tokio::test]
async fn an_appeal_after_the_window_is_refused_naming_the_deadline() {
    let harness = Harness::new("after").await;
    let (mut reviewer, task_id, _account, target) =
        harness.assigned_task_with_target("after").await;

    let (status, body) = reviewer
        .post(
            &format!("/api/v1/moderation/tasks/{task_id}/decide"),
            json!({
                "outcome": "uphold",
                "ground": "spam",
                "facts": "Six identical promotional replies within one hour.",
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{status} {body}");
    let statement_id = body["statement_id"]
        .as_str()
        .expect("statement id")
        .to_owned();
    let sanction = sanction_from_statement(&mut reviewer, &target, &statement_id).await;

    // The window closed yesterday — set on the stored row, because that is the only way it can close: the deadline was
    // written with the decision and nothing recomputes it.
    harness
        .exec(&format!(
            "UPDATE statement_of_reasons SET appeal_until = '2020-01-01T00:00:00Z' WHERE id = '{statement_id}'"
        ))
        .await;

    let mut owner = harness.client();
    let (status, body) = owner
        .post(
            "/api/v1/auth/login",
            json!({ "email": "after-target@example.test", "password": PASSWORD }),
        )
        .await;
    assert!(
        status == StatusCode::OK || status == StatusCode::CREATED,
        "sign in: {status} {body}"
    );

    let (status, body) = owner
        .post(
            "/api/v1/appeals",
            json!({ "sanction_id": sanction, "statement": "I would like this reviewed." }),
        )
        .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{status} {body}");
    let refusal = body["error"]["field_errors"]["appeal_until"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(
        refusal.contains("2020-01-01T00:00:00Z"),
        "the refusal names the deadline it passed: {body}"
    );
}

/// The transparency summary is a count, and a count below the threshold is not published.
///
/// The decisions are seeded directly: the *writer* is what the tests above prove, and re-deciding seven reports to reach a
/// number would test the queue again rather than the report.
#[tokio::test]
async fn the_summary_counts_grounds_and_never_names_an_account() {
    let harness = Harness::new("summary").await;
    let seed = |ground: &'static str, legal: &'static str, n: usize| {
        let harness = &harness;
        async move {
            for _ in 0..n {
                harness
                    .exec(&format!(
                        "INSERT INTO statement_of_reasons \
                         (id, task_id, report_id, outcome, ground, facts, legal_ground, decided_by, decided_at, appeal_until) \
                         VALUES ('{}', '{}', NULL, 'uphold', '{ground}', 'f', '{legal}', 'reviewer-account', \
                                 '2026-11-01T00:00:00Z', '2026-12-01T00:00:00Z')",
                        uuid::Uuid::new_v4(),
                        uuid::Uuid::new_v4()
                    ))
                    .await;
            }
        }
    };
    seed("spam", "", 6).await;
    seed("copyright", "17 U.S.C. 512", 2).await;

    let mut anonymous = harness.client();
    let (status, body) = anonymous.get("/api/v1/statements/summary").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a transparency report is public: {status} {body}"
    );
    assert_eq!(body["min_count"], 5);

    let grounds = body["grounds"].as_array().expect("grounds");
    let spam = grounds
        .iter()
        .find(|g| g["ground"] == "spam")
        .expect("the spam ground is reported");
    let upheld = spam["outcomes"]
        .as_array()
        .expect("outcomes")
        .iter()
        .find(|o| o["outcome"] == "uphold")
        .expect("the uphold outcome is reported");
    assert_eq!(upheld["count"], 6, "six decisions are six: {upheld}");
    assert_eq!(upheld["suppressed"], false);

    let copyright = grounds
        .iter()
        .find(|g| g["ground"] == "copyright")
        .expect("the copyright ground is reported");
    let two = copyright["outcomes"]
        .as_array()
        .expect("outcomes")
        .iter()
        .find(|o| o["outcome"] == "uphold")
        .expect("the uphold outcome is reported");
    assert_eq!(
        two["count"],
        Value::Null,
        "a count of two is two people: {two}"
    );
    assert_eq!(two["suppressed"], true);
    assert!(
        two["reason"].as_str().unwrap_or_default().contains("5"),
        "the cell says which threshold it fell under: {two}"
    );

    // The report contains no statement, no account and no report id — the reason it is a report at all.
    let text = body.to_string();
    assert!(
        !text.contains("reviewer-account"),
        "no account appears: {text}"
    );
    assert!(!text.contains("task_id"), "no task id appears");
    assert!(!text.contains("statement_id"), "no statement id appears");
}

/// The person whose account was acted on is told, with the reasons and the deadline.
///
/// Before this, the reporter got an answer and the author got a sanction with no explanation. The notification is the
/// half of the row that makes the statement reach anybody.
#[tokio::test]
async fn the_author_is_told_when_their_account_is_acted_on() {
    let harness = Harness::new("told").await;
    let (mut reviewer, task_id, _account, target) = harness.assigned_task_with_target("told").await;

    let facts = "Six identical promotional replies within one hour, from a new account.";
    let (status, body) = reviewer
        .post(
            &format!("/api/v1/moderation/tasks/{task_id}/decide"),
            json!({ "outcome": "uphold", "ground": "spam", "facts": facts }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{status} {body}");
    let appeal_until = body["appeal_until"].as_str().expect("deadline").to_owned();

    // The author signs in and reads their own inbox.
    let mut author = harness.client();
    let (status, body) = author
        .post(
            "/api/v1/auth/login",
            json!({ "email": "told-target@example.test", "password": PASSWORD }),
        )
        .await;
    assert!(status == StatusCode::OK, "sign in: {status} {body}");

    let (status, body) = author.get("/api/v1/notifications").await;
    assert_eq!(status, StatusCode::OK, "{status} {body}");
    let items = body["items"]
        .as_array()
        .or_else(|| body["notifications"].as_array())
        .or_else(|| body.as_array())
        .unwrap_or_else(|| panic!("a list of notifications: {body}"));
    let decision = items
        .iter()
        .find(|n| n["kind"] == "moderation_decision")
        .unwrap_or_else(|| panic!("the author is told: {body}"));
    let text = decision["body"].as_str().unwrap_or_default();
    assert!(
        text.contains(facts),
        "the reasons are carried verbatim: {text}"
    );
    assert!(text.contains("spam"), "the ground is named: {text}");
    assert!(
        text.contains(&appeal_until),
        "and the deadline they can appeal by: {text}"
    );
    assert!(
        decision["work_id"].is_null(),
        "an account sanction links to no work: {decision}"
    );
    let _ = target;
}

/// A copyright notice missing a statutory element is not a weak notice — it is not a notice.
#[tokio::test]
async fn a_dmca_notice_missing_a_statutory_element_is_refused() {
    let harness = Harness::new("dmca").await;
    let mut reporter = harness.client();
    reporter
        .register("dmca-reporter@example.test", "Complainant")
        .await;

    // 17 U.S.C. 512(c)(3) lists four; each one missing is refused by the field's own name.
    for field in [
        "copyrighted_work",
        "infringing_material",
        "good_faith_statement",
        "signature",
    ] {
        let mut payload = json!({
            "subject_type": "work",
            "subject_id": "w-1",
            "reason": "copyright",
            "kind": "dmca",
            "copyrighted_work": "The Long Winter, chapter 3",
            "infringing_material": "Every paragraph of chapter 3, copied verbatim",
            "good_faith_statement": "I have a good faith belief that this use is not authorised.",
            "signature": "A. Complainant",
        });
        payload[field] = json!("");
        let (status, body) = reporter.post("/api/v1/reports", payload).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "a notice without `{field}` is refused: {status} {body}"
        );
        assert!(
            body["error"]["field_errors"].get(field).is_some(),
            "the refusal names `{field}`: {body}"
        );
    }

    // With all four, the notice is accepted and the statutory record is stored beside the report.
    let (status, body) = reporter
        .post(
            "/api/v1/reports",
            json!({
                "subject_type": "work",
                "subject_id": "w-1",
                "reason": "copyright",
                "kind": "dmca",
                "copyrighted_work": "The Long Winter, chapter 3",
                "infringing_material": "Every paragraph of chapter 3, copied verbatim",
                "good_faith_statement": "I have a good faith belief that this use is not authorised.",
                "signature": "A. Complainant",
            }),
        )
        .await;
    assert!(
        status == StatusCode::OK || status == StatusCode::CREATED,
        "a complete notice is filed: {status} {body}"
    );
    let report_id = body["id"].as_str().expect("report id").to_owned();
    use sqlx::Row;
    // The two engines have two row types, so the read happens inside each arm — a `match` over them cannot unify.
    let signature = match harness.db().backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query("SELECT signature FROM dmca_notices WHERE report_id = ?")
                .bind(&report_id)
                .fetch_optional(harness.db().sqlite_pool().expect("sqlite"))
                .await
                .expect("query the notice")
                .map(|row| row.get::<String, _>("signature"))
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query("SELECT signature FROM dmca_notices WHERE report_id = $1")
                .bind(&report_id)
                .fetch_optional(harness.db().postgres_pool().expect("postgres"))
                .await
                .expect("query the notice")
                .map(|row| row.get::<String, _>("signature"))
        }
    };
    assert_eq!(
        signature.as_deref(),
        Some("A. Complainant"),
        "the statutory record exists beside the report"
    );

    // And a DSA notice still needs none of those fields: the two regimes stayed two forms.
    let (status, body) = reporter
        .post(
            "/api/v1/reports",
            json!({ "subject_type": "work", "subject_id": "w-2", "reason": "spam" }),
        )
        .await;
    assert!(
        status == StatusCode::OK || status == StatusCode::CREATED,
        "a DSA notice is unchanged by the DMCA arm: {status} {body}"
    );
}

/// **The door the author was missing.** Step 3 told them about the decision in a notification; this is where they can re-read it before
/// appealing, and a stranger learns only that no statement exists.
#[tokio::test]
async fn the_person_the_decision_is_about_can_read_the_statement() {
    let harness = Harness::new("readdoor").await;
    let (mut reviewer, task_id, _reviewer_account, target) =
        harness.assigned_task_with_target("readdoor").await;

    let (status, body) = reviewer
        .post(
            &format!("/api/v1/moderation/tasks/{task_id}/decide"),
            json!({
                "outcome": "uphold",
                "ground": "spam",
                "facts": "Six identical promotional replies within one hour.",
            }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "decide with reasons: {status} {body}"
    );
    let appeal_until = body["appeal_until"].as_str().expect("deadline").to_owned();

    // The affected account signs in and reads what was decided about them.
    let mut subject = harness.client();
    let (status, login) = subject
        .request(
            "POST",
            "/api/v1/auth/login",
            Some(json!({ "email": "readdoor-target@example.test", "password": PASSWORD })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "the subject signs in: {login}");

    let (status, statement) = subject.get(&format!("/api/v1/statements/{task_id}")).await;
    assert_eq!(status, StatusCode::OK, "{statement}");
    assert_eq!(statement["ground"], "spam", "{statement}");
    assert_eq!(statement["outcome"], "uphold", "{statement}");
    assert_eq!(
        statement["appeal_until"], appeal_until,
        "the deadline they must act on is in the notice they can re-read: {statement}"
    );
    assert!(
        statement["facts"]
            .as_str()
            .unwrap_or_default()
            .contains("identical promotional replies"),
        "{statement}"
    );

    // A stranger gets 404, not 403: whether a statement exists about an account is information.
    let mut stranger = harness.client();
    stranger
        .register("readdoor-stranger@example.test", "Stranger")
        .await;
    let (status, refused) = stranger.get(&format!("/api/v1/statements/{task_id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{refused}");
    assert!(
        refused["error"]["message"]
            .as_str()
            .or_else(|| refused["error"]["code"].as_str())
            .unwrap_or_default()
            .to_lowercase()
            .contains("not"),
        "and the refusal is the same shape as a statement that does not exist: {refused}"
    );
    let _ = target;
}
