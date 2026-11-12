//! M45-55 — compliance tooling over HTTP: the forms, the templates, and the operator's audited subject-access read.
//!
//! Three properties, one per door:
//!
//! 1. **The forms are public and match what `POST /reports` enforces.** A complainant needs the form before they have an
//!    account, and a form that disagrees with the validator is a refusal the UI never warned about.
//! 2. **The templates are operator-only and cover every ground.** A ground with no template is a decision written from
//!    nothing, and the closed vocabulary means the missing one is knowable.
//! 3. **An operator's subject-access read names its legal basis and is recorded.** A read that cannot be recorded is a read
//!    that must not happen — the audit append failing refuses the disclosure rather than answering it silently.

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
        "darkhold-m45-55-{tag}-{}-{:?}",
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

    async fn get(&mut self, uri: &str) -> (StatusCode, Value) {
        self.request("GET", uri, None).await
    }

    /// The same GET, keeping the headers — a download is a body *and* a filename.
    async fn get_with_headers(&mut self, uri: &str) -> (StatusCode, axum::http::HeaderMap, Value) {
        let mut builder = Request::builder().method("GET").uri(uri);
        for (name, value) in &self.cookies {
            builder = builder.header(header::COOKIE, format!("{name}={value}"));
        }
        let response = self
            .app
            .clone()
            .oneshot(builder.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            status,
            headers,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn post(&mut self, uri: &str, body: Value) -> (StatusCode, Value) {
        self.request("POST", uri, Some(body)).await
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

    /// An account with the standing a senior reviewer has, **and the operator role**.
    ///
    /// The role is new, and it is the point of M45-55's DSAR door: `subject_access` used to be gated by trust level alone, so this helper's
    /// trust-6 account could read anybody's personal data. It is now `require_admin_of`, so the helper grants what the door asks for. The role comes
    /// from `config.administration.operator_account_id` rather than a session role because roles are baked into the session row at login and this
    /// client registers rather than signs in — the config path is the break-glass identity ADR 0030 defines for exactly this.
    async fn operator(&self, tag: &str) -> (Client, String) {
        let mut client = self.client();
        let account = client
            .register(&format!("{tag}-operator@example.test"), "Operator")
            .await;
        darkhold_db::governance::set_trust(self.db(), &account, 6, "test")
            .await
            .expect("grant standing");
        let mut config = self.config.clone();
        config.administration.operator_account_id = Some(account.parse().expect("account uuid"));
        let mut operator = Client::new(server::build_router(AppState::new(
            config,
            self.db().clone(),
        )));
        operator.cookies = client.cookies.clone();
        (operator, account)
    }

    async fn subject(&self, tag: &str) -> String {
        let mut client = self.client();
        client
            .register(&format!("{tag}-subject@example.test"), "Subject")
            .await
    }

    async fn count_audit(&self, account: &str) -> i64 {
        use sqlx::Row;
        match self.db().backend() {
            darkhold_db::Backend::Sqlite => sqlx::query(
                "SELECT COUNT(*) AS n FROM audit_log WHERE action = 'subject.access' AND subject_id = ?",
            )
            .bind(account)
            .fetch_one(self.db().sqlite_pool().expect("sqlite"))
            .await
            .expect("count audit")
            .get::<i64, _>("n"),
            darkhold_db::Backend::Postgres => sqlx::query(
                "SELECT COUNT(*) AS n FROM audit_log WHERE action = 'subject.access' AND subject_id = $1",
            )
            .bind(account)
            .fetch_one(self.db().postgres_pool().expect("postgres"))
            .await
            .expect("count audit")
            .get::<i64, _>("n"),
        }
    }
}

#[tokio::test]
async fn the_notice_forms_are_public_and_name_what_the_validator_requires() {
    let harness = Harness::new("forms").await;
    let mut anonymous = harness.client();

    let (status, body) = anonymous.get("/api/v1/notices/forms").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a complainant needs the form before an account: {status} {body}"
    );

    let forms = body["forms"].as_array().expect("forms");
    assert_eq!(forms.len(), 2, "two regimes, two forms");

    let dsa = forms
        .iter()
        .find(|f| f["kind"] == "dsa")
        .expect("the DSA form");
    assert!(
        dsa["legal_basis"]
            .as_str()
            .unwrap_or_default()
            .contains("Article 16"),
        "the DSA form names its article: {dsa}"
    );

    let dmca = forms
        .iter()
        .find(|f| f["kind"] == "dmca")
        .expect("the DMCA form");
    let names: Vec<&str> = dmca["fields"]
        .as_array()
        .expect("fields")
        .iter()
        .filter(|f| f["required"] == true)
        .filter_map(|f| f["name"].as_str())
        .collect();
    for required in [
        "copyrighted_work",
        "infringing_material",
        "good_faith_statement",
        "signature",
    ] {
        assert!(
            names.contains(&required),
            "the DMCA form asks for `{required}` because `POST /reports` refuses without it: {names:?}"
        );
    }
}

#[tokio::test]
async fn the_templates_are_operator_only_and_cover_every_ground() {
    let harness = Harness::new("templates").await;

    let mut anonymous = harness.client();
    let (status, _body) = anonymous.get("/api/v1/statements/templates").await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "the instance's voice is not public"
    );

    let (mut operator, _account) = harness.operator("templates").await;
    let (status, body) = operator.get("/api/v1/statements/templates").await;
    assert_eq!(status, StatusCode::OK, "{status} {body}");

    let templates = body["templates"].as_array().expect("templates");
    assert_eq!(
        templates.len(),
        darkhold_domain::governance::StatementGround::ALL.len(),
        "every ground has a template: {body}"
    );
    for ground in darkhold_domain::governance::StatementGround::ALL {
        let entry = templates
            .iter()
            .find(|t| t["ground"] == ground.as_str())
            .unwrap_or_else(|| panic!("{} has no template", ground.as_str()));
        assert!(
            entry["template"]
                .as_str()
                .unwrap_or_default()
                .contains("{facts}"),
            "{} template carries the reviewer's facts: {entry}",
            ground.as_str()
        );
        let placeholders: Vec<String> = entry["placeholders"]
            .as_array()
            .expect("placeholders")
            .iter()
            .filter_map(|p| p.as_str().map(str::to_owned))
            .collect();
        assert_eq!(
            placeholders.iter().any(|p| p == "legal_ground"),
            ground.requires_legal_ground(),
            "{} and its placeholders agree about naming law",
            ground.as_str()
        );
    }
}

#[tokio::test]
async fn an_operator_read_needs_a_basis_and_is_recorded() {
    let harness = Harness::new("dsar").await;
    let (mut operator, _account) = harness.operator("dsar").await;
    let subject = harness.subject("dsar").await;

    // No basis: refused by name, and nothing was read.
    let (status, body) = operator
        .get(&format!("/api/v1/admin/subjects/{subject}/data"))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{status} {body}");
    assert!(
        body["error"]["field_errors"].get("basis").is_some(),
        "the refusal names `basis`: {body}"
    );
    assert_eq!(
        harness.count_audit(&subject).await,
        0,
        "a refused read leaves no receipt"
    );

    // An invented basis is refused for the same reason.
    let (status, body) = operator
        .get(&format!(
            "/api/v1/admin/subjects/{subject}/data?basis=curiosity"
        ))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{status} {body}");
    assert!(
        body["error"]["field_errors"].get("basis").is_some(),
        "{body}"
    );

    // A real basis discloses, and the receipt is written.
    let (status, body) = operator
        .get(&format!(
            "/api/v1/admin/subjects/{subject}/data?basis=legal_obligation"
        ))
        .await;
    assert_eq!(status, StatusCode::OK, "{status} {body}");
    assert_eq!(body["basis"], "legal_obligation");
    assert_eq!(
        body["subject"]["account_id"], subject,
        "the disclosure is the subject's own data, assembled by the route the reader uses"
    );
    assert_eq!(
        harness.count_audit(&subject).await,
        1,
        "one successful read, one receipt"
    );

    // A subject-access answer is a document: handed over, archived, produced again on request.
    let (status, headers, _body) = operator
        .get_with_headers(&format!(
            "/api/v1/admin/subjects/{subject}/data?basis=legal_obligation"
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    let disposition = headers
        .get(header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    assert!(
        disposition.starts_with("attachment") && disposition.contains(&format!("dsar-{subject}-")),
        "the read arrives as a named file: {disposition}"
    );
    assert_eq!(
        harness.count_audit(&subject).await,
        2,
        "and it is a second read, so a second receipt — the count is per act, not per account"
    );

    // And a reader without the standing is refused before any of that.
    let mut reader = harness.client();
    reader.register("dsar-reader@example.test", "Reader").await;
    let (status, _body) = reader
        .get(&format!(
            "/api/v1/admin/subjects/{subject}/data?basis=legal_obligation"
        ))
        .await;
    // `404`, not `403`: the door is operator-only now, and ADR 0030 answers a caller with no admin standing at all with 404 — confirming that a
    // subject-access surface exists is itself a disclosure. `403`/`422` were what a trust-level refusal looked like when the gate was a level.
    assert!(
        status == StatusCode::NOT_FOUND
            || status == StatusCode::FORBIDDEN
            || status == StatusCode::UNPROCESSABLE_ENTITY,
        "a plain reader cannot read another account's data: {status}"
    );
    assert_eq!(
        harness.count_audit(&subject).await,
        2,
        "and the refusal wrote nothing — the two receipts are the two successful reads"
    );
}

/// A senior moderator is not an operator, and a DSAR read is the operator's.
///
/// The door used to be gated by `Privilege::ResolveReports`, so trust level alone opened it: a trust-6 account could read a stranger's assembled
/// personal data with a legal basis it typed itself. This test holds the separation — trust 9, no operator role, `404` — because that is the
/// distinction a trust-level gate cannot express and the audit receipt cannot repair after the fact.
#[tokio::test]
async fn a_high_trust_moderator_cannot_read_a_subject() {
    let harness = Harness::new("dsar-moderator").await;
    let subject = harness.subject("target").await;
    let mut moderator = harness.client();
    let moderator_account = moderator
        .register("senior-moderator@example.test", "SeniorModerator")
        .await;
    darkhold_db::governance::set_trust(harness.db(), &moderator_account, 9, "test")
        .await
        .expect("grant trust");

    let (status, body) = moderator
        .get(&format!(
            "/api/v1/admin/subjects/{subject}/data?basis=legal_obligation"
        ))
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "trust is not standing: {status} {body}"
    );
    assert_eq!(
        harness.count_audit(&subject).await,
        0,
        "a refused read leaves no receipt because nothing was read: {body}"
    );
}
