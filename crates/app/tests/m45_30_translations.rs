//! M45-30 — §61 cross-language supply through the real router.
//!
//! What only the route can get wrong:
//!
//! - the supply report is public, and it says out loud that taste transfer is unavailable here;
//! - **§40.2's enforcement sentence is finally true**: `POST /works/{id}/translations` refuses a work
//!   whose `translation` statement is not `yes` — measured before this milestone, the door created a
//!   job without reading the statement at all;
//! - funding needs a session, a real language tag, a positive amount, and permission **before**
//!   money: a refusal must not tell a patron to top up for something the author forbade;
//! - the bounty door writes `job_kind = 'translation'` and nothing else.

use std::path::PathBuf;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::Backend;
use darkhold_domain::permission::{Permission, PermissionStatement};
use serde_json::{json, Value};
use test_support::TestDb;
use tower::ServiceExt;

const PASSWORD: &str = "correct horse battery staple 7";
const T0: &str = "2026-03-01T00:00:00Z";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-m45-30-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
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
        if !self.cookies.is_empty() {
            let jar = self
                .cookies
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("; ");
            builder = builder.header(header::COOKIE, jar);
            if let Some((_, token)) = self.cookies.iter().find(|(k, _)| k == "darkhold_csrf") {
                builder = builder.header("x-csrf-token", token);
            }
        }
        let body = match body {
            Some(v) => Body::from(v.to_string()),
            None => Body::empty(),
        };
        let response = self
            .app
            .clone()
            .oneshot(builder.body(body).expect("request"))
            .await
            .expect("response");
        let status = response.status();
        self.capture(&response);
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("body");
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        };
        (status, value)
    }

    async fn get(&mut self, uri: &str) -> (StatusCode, Value) {
        self.request("GET", uri, None).await
    }

    async fn post(&mut self, uri: &str, body: Value) -> (StatusCode, Value) {
        self.request("POST", uri, Some(body)).await
    }

    async fn register(&mut self, email: &str, handle: &str) -> (String, String) {
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
        let (_, me) = self.get("/api/v1/auth/me").await;
        (
            me["account"]["id"].as_str().expect("account id").to_owned(),
            me["pseuds"][0]["id"].as_str().expect("pseud id").to_owned(),
        )
    }

    async fn activate(&mut self, pseud: &str) {
        let (status, body) = self
            .post(&format!("/api/v1/pseuds/{pseud}/activate"), json!({}))
            .await;
        assert!(
            status == StatusCode::NO_CONTENT || status == StatusCode::OK,
            "activate: {status} {body}"
        );
    }
}

struct Harness {
    _dir: PathBuf,
    tdb: TestDb,
    config: Config,
}

impl Harness {
    async fn new(tag: &str) -> Self {
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
}

async fn seed_work(h: &Harness, pseud: &str, language: &str, translation: Permission) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    let db = h.tdb.db();
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO works (id, owner_pseud_id, title, language, created_at, updated_at,
                                    generated_content_posture)
                 VALUES (?, ?, 'A work', ?, ?, ?, 'forbid')",
            )
            .bind(&id)
            .bind(pseud)
            .bind(language)
            .bind(T0)
            .bind(T0)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("work");
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO works (id, owner_pseud_id, title, language, created_at, updated_at,
                                    generated_content_posture)
                 VALUES ($1::uuid, $2::uuid, 'A work', $3, $4::timestamptz, $4::timestamptz, 'forbid')",
            )
            .bind(uuid::Uuid::parse_str(&id).expect("uuid"))
            .bind(uuid::Uuid::parse_str(pseud).expect("uuid"))
            .bind(language)
            .bind(T0)
            .execute(db.postgres_pool().expect("postgres"))
            .await
            .expect("work");
        }
    }
    let mut stmt = PermissionStatement::unstated();
    stmt.translation = translation;
    darkhold_db::permission::set_work_permission_statement(db, &id, &stmt)
        .await
        .expect("statement");
    id
}

#[tokio::test]
async fn the_supply_report_is_public_and_names_the_missing_transfer_pieces() {
    let h = Harness::new("supply").await;
    let mut anon = h.client();
    let (status, body) = anon.get("/api/v1/translations/supply").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["transfer"]["available"], json!(false), "{body}");
    let missing = body["transfer"]["missing"].as_array().expect("missing");
    assert_eq!(missing.len(), 2, "{body}");
    assert!(
        missing.iter().any(|m| m == "stylometry"),
        "the report names stylometry as missing: {body}"
    );
    assert!(
        body["by_language"].is_object() && body["total"].is_number(),
        "a report is counts and a total: {body}"
    );
    assert!(
        body["unrepresented"].is_array(),
        "the report says which languages are missing: {body}"
    );
}

#[tokio::test]
async fn creating_a_translation_is_refused_unless_the_author_said_yes() {
    let h = Harness::new("gate").await;
    let mut author = h.client();
    let (_account, pseud) = author.register("author@example.invalid", "author").await;
    author.activate(&pseud).await;

    for (index, (permission, expected_word)) in [
        (Permission::No, "no"),
        (Permission::Unstated, "unstated"),
        (Permission::Ask, "consulted"),
    ]
    .into_iter()
    .enumerate()
    {
        let work_id = seed_work(&h, &pseud, "en", permission).await;
        let mut requester = h.client();
        let handle = format!("requester{index}");
        let (_a, p) = requester
            .register(&format!("{handle}@example.invalid"), &handle)
            .await;
        requester.activate(&p).await;
        let (status, body) = requester
            .post(
                &format!("/api/v1/works/{work_id}/translations"),
                json!({ "source_lang": "en", "target_lang": "pt", "provider": "human" }),
            )
            .await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{permission:?}: {body}"
        );
        let reason = body["error"]["field_errors"]["translation"]
            .as_str()
            .expect("refusal on the field `translation`");
        assert!(
            reason.contains(expected_word),
            "{permission:?} refusal should mention {expected_word:?}: {reason:?}"
        );
    }

    // And `yes` creates the job.
    let work_id = seed_work(&h, &pseud, "en", Permission::Yes).await;
    let (status, body) = author
        .post(
            &format!("/api/v1/works/{work_id}/translations"),
            json!({ "source_lang": "en", "target_lang": "pt", "provider": "human" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["id"].is_string(), "{body}");
}

#[tokio::test]
async fn funding_needs_a_session_and_a_target_language() {
    let h = Harness::new("fundshape").await;
    let mut anon = h.client();
    let work_id = uuid::Uuid::new_v4().to_string();
    let (status, body) = anon
        .post(
            &format!("/api/v1/works/{work_id}/translation-bounty"),
            json!({ "target_language": "pt", "amount": 1000 }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"]["field_errors"]["session"].is_string(),
        "{body}"
    );

    let mut patron = h.client();
    let (_a, p) = patron.register("patron0@example.invalid", "patron0").await;
    patron.activate(&p).await;
    let (status, body) = patron
        .post(
            &format!("/api/v1/works/{work_id}/translation-bounty"),
            json!({ "target_language": "  ", "amount": 1000 }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"]["field_errors"]["target_language"].is_string(),
        "an empty language tag is refused on its own field: {body}"
    );

    // A non-positive amount is a promise of nothing: refused on its own field.
    let (status, body) = patron
        .post(
            &format!("/api/v1/works/{work_id}/translation-bounty"),
            json!({ "target_language": "pt", "amount": 0 }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"]["field_errors"]["amount"].is_string(),
        "a zero-credit bounty is refused on `amount`: {body}"
    );
}

#[tokio::test]
async fn permission_is_checked_before_money() {
    let h = Harness::new("order").await;
    let mut author = h.client();
    let (_a, pseud) = author.register("author2@example.invalid", "author2").await;
    author.activate(&pseud).await;
    let work_id = seed_work(&h, &pseud, "en", Permission::No).await;

    // An amount larger than any balance this account could hold: if the balance were read first the
    // refusal would land on `amount`, so the field that answers is the whole proof of the order.
    let mut patron = h.client();
    let (_b, p) = patron.register("patron2@example.invalid", "patron2").await;
    patron.activate(&p).await;
    let (status, body) = patron
        .post(
            &format!("/api/v1/works/{work_id}/translation-bounty"),
            json!({ "target_language": "pt-BR", "amount": 999_999_999i64 }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let reason = body["error"]["field_errors"]["translation"]
        .as_str()
        .expect("refusal on the field `translation`");
    assert!(
        reason.contains("no"),
        "the author's statement is named before any balance: {reason:?}"
    );
    assert!(
        body["error"]["field_errors"]["amount"].is_null(),
        "the balance is read second, so it cannot be what refuses: {body}"
    );
    let (_, listing) = patron.get("/api/v1/translations/bounties").await;
    assert_eq!(listing["count"], json!(0), "nothing was written: {listing}");
}

#[tokio::test]
async fn a_funded_translation_appears_in_the_listing_with_its_terms() {
    let h = Harness::new("funded").await;
    let mut author = h.client();
    let (account, pseud) = author.register("author3@example.invalid", "author3").await;
    author.activate(&pseud).await;
    let work_id = seed_work(&h, &pseud, "es-419", Permission::Yes).await;

    // Credit the account through the economy's own ledger.
    let db = h.tdb.db();
    let txn = uuid::Uuid::new_v4().to_string();
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO credit_transactions (id, type, idempotency_key, reference, created_at)
                 VALUES (?, 'grant', ?, 'test', ?)",
            )
            .bind(&txn)
            .bind(&txn)
            .bind(T0)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("txn");
            sqlx::query(
                "INSERT INTO credit_entries (transaction_id, account, bucket, amount_bp, created_at)
                 VALUES (?, ?, 'earned', 100000, ?)",
            )
            .bind(&txn)
            .bind(&account)
            .bind(T0)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("entry");
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO credit_transactions (id, type, idempotency_key, reference, created_at)
                 VALUES ($1, 'grant', $2, 'test', $3::timestamptz)",
            )
            .bind(&txn)
            .bind(&txn)
            .bind(T0)
            .execute(db.postgres_pool().expect("postgres"))
            .await
            .expect("txn");
            sqlx::query(
                "INSERT INTO credit_entries (transaction_id, account, bucket, amount_bp, created_at)
                 VALUES ($1, $2, 'earned', 100000, $3::timestamptz)",
            )
            .bind(&txn)
            .bind(&account)
            .bind(T0)
            .execute(db.postgres_pool().expect("postgres"))
            .await
            .expect("entry");
        }
    }

    let (status, body) = author
        .post(
            &format!("/api/v1/works/{work_id}/translation-bounty"),
            json!({ "target_language": "pt-BR", "amount": 10_000 }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["funded"], json!(true));
    assert_eq!(
        body["target_language"],
        json!("pt"),
        "the language is normalised before it is stored: {body}"
    );

    let (_, listing) = author.get("/api/v1/translations/bounties").await;
    assert_eq!(listing["count"], json!(1), "{listing}");
    let terms = listing["bounties"][0]["terms"].as_str().expect("terms");
    let terms: Value = serde_json::from_str(terms).expect("json");
    assert_eq!(
        terms["source_language"],
        json!("es"),
        "the work's own language"
    );
    assert_eq!(terms["target_language"], json!("pt"));
}

#[tokio::test]
async fn the_module_row_is_registered_enabled_and_names_both_sections() {
    let h = Harness::new("module").await;
    let mut anon = h.client();
    let (status, body) = anon.get("/api/v1/modules").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let row = body["modules"]
        .as_array()
        .expect("modules")
        .iter()
        .find(|m| m["id"] == json!("translation"))
        .unwrap_or_else(|| panic!("translation is not registered"));
    assert_eq!(row["enabled"], json!(true));
    let spec = row["spec"].as_str().expect("spec");
    assert!(
        spec.contains("§61") && spec.contains("§40.2"),
        "the row names the sections it now implements: {spec:?}"
    );
}
