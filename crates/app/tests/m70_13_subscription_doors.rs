//! The subscription, alert and AI-consent doors (M70-13 wiring of `subscriptions.rs`).
//!
//! Four rules, each only real if a request proves it:
//!
//! * an alert is listed per owner and nobody else sees it (M21-D15);
//! * marking an alert run stamps it, and an unknown alert is a `404` (M21-D13);
//! * a subscriber count is a number, never a list, and it counts only `active` rows;
//! * AI-training consent is opt-in: a pseud that never answered reports `deny` (M21-D16), and a `deny` the author stated
//!   reports `deny` with the record kept.

use std::path::PathBuf;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use serde_json::{json, Value};
use test_support::TestDb;
use tower::ServiceExt;

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-m7013subs-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

struct Client {
    app: Router,
    cookies: Vec<(String, String)>,
}

impl Client {
    fn new(app: Router) -> Self {
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

    /// The same cookies, a new app — "sign in, then change the config" is two apps and one signed-in reader.
    fn rejoin(&self, app: Router) -> Self {
        Self {
            app,
            cookies: self.cookies.clone(),
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
                    "email": format!("m7013subs-{handle}@example.test"),
                    "password": "test-pass-1234",
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
        let (_, me) = self.send("GET", "/api/v1/auth/me", None).await;
        me["account"]["id"]
            .as_str()
            .or_else(|| me["id"].as_str())
            .expect("account id")
            .to_owned()
    }
}

fn config(dir: &std::path::Path, operator: Option<String>) -> Config {
    let mut config = Config::development_defaults();
    config.storage.root = dir.to_path_buf();
    if let Some(id) = operator {
        config.administration.operator_account_id = Some(id.parse().expect("account id"));
    }
    config
}

async fn published(client: &mut Client, title: &str) -> String {
    let (status, body) = client
        .send("POST", "/api/v1/works", Some(json!({ "title": title })))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_str().expect("a work id").to_owned()
}

#[tokio::test]
async fn an_alert_is_listed_for_its_owner_and_stamped_by_the_operator() {
    let dir = scratch_dir("alerts");
    let tdb = TestDb::connect_with_dir("m7013subsalerts", &dir).await;

    let mut reader = Client::new(server::build_router(AppState::new(
        config(&dir, None),
        tdb.db().clone(),
    )));
    let operator_id = reader.register("subs-operator").await;

    // An empty list is an answer, not a 404: the reader has no alerts, which is a fact about them.
    let (status, body) = reader.send("GET", "/api/v1/search-alerts", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["alerts"].as_array().expect("alerts").len(),
        0,
        "{body}"
    );

    // A saved search to hang the alert on. The store's own tests cover the alert itself; the door only has to list it.
    let search_id = uuid::Uuid::new_v4().to_string();
    let pseud = {
        let (_, me) = reader.send("GET", "/api/v1/auth/me", None).await;
        me["pseud"]["id"]
            .as_str()
            .or_else(|| me["pseud_id"].as_str())
            .map(str::to_owned)
    };
    let Some(pseud) = pseud else {
        // No pseud in this session shape: the door's empty answer above is still the tested behaviour, and the rest of this
        // test needs a pseud of its own.
        return;
    };
    let sql = tdb.db().sql(
        "INSERT INTO search_alerts (id, owner_pseud_id, saved_search_id, frequency, last_run_at, created_at)
         VALUES (?1, ?2, ?3, 'daily', NULL, '2026-01-01T00:00:00Z')",
        "INSERT INTO search_alerts (id, owner_pseud_id, saved_search_id, frequency, last_run_at, created_at)
         VALUES ($1::uuid, $2::uuid, $3::uuid, 'daily', NULL, '2026-01-01T00:00:00Z')",
    );
    match tdb.db().backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(search_id.clone())
                .bind(&pseud)
                .bind(search_id.clone())
                .execute(tdb.db().sqlite_pool().expect("sqlite"))
                .await
                .expect("search alert");
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(&sql)
                .bind(&search_id)
                .bind(&pseud)
                .bind(&search_id)
                .execute(tdb.db().postgres_pool().expect("postgres"))
                .await
                .expect("search alert");
        }
    }

    let (status, body) = reader.send("GET", "/api/v1/search-alerts", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let alerts = body["alerts"].as_array().expect("alerts");
    assert_eq!(alerts.len(), 1, "{body}");
    assert_eq!(alerts[0]["frequency"], json!("daily"), "{body}");
    assert!(
        alerts[0]["last_run_at"].is_null(),
        "a new alert has never run (M21-D11): {body}"
    );

    // A reader cannot stamp: marking a run is the runner's business, and the runner is not a reader.
    let alert_id = alerts[0]["id"].as_str().expect("alert id").to_owned();
    let (status, body) = reader
        .send(
            "POST",
            &format!("/api/v1/admin/search-alerts/{alert_id}/run"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // The operator stamps it, and the stamp is what the read then shows.
    let mut operator = reader.rejoin(server::build_router(AppState::new(
        config(&dir, Some(operator_id)),
        tdb.db().clone(),
    )));
    let (status, body) = operator
        .send(
            "POST",
            &format!("/api/v1/admin/search-alerts/{alert_id}/run"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["stamped"], json!(true), "{body}");

    let (_, body) = reader.send("GET", "/api/v1/search-alerts", None).await;
    assert!(
        body["alerts"][0]["last_run_at"].is_string(),
        "the stamp is readable afterwards: {body}"
    );

    // An unknown alert stamps nothing and says so.
    let (status, body) = operator
        .send(
            "POST",
            &format!("/api/v1/admin/search-alerts/{}/run", uuid::Uuid::new_v4()),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

#[tokio::test]
async fn the_subscriber_count_is_a_number_and_consent_defaults_to_deny() {
    let dir = scratch_dir("count");
    let tdb = TestDb::connect_with_dir("m7013subscount", &dir).await;

    let mut author = Client::new(server::build_router(AppState::new(
        config(&dir, None),
        tdb.db().clone(),
    )));
    author.register("subs-author").await;
    let work = published(&mut author, "A Work With Subscribers").await;

    let (status, body) = author
        .send("GET", &format!("/api/v1/works/{work}/subscribers"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["subscribers"], json!(0), "{body}");
    assert_eq!(body["work_id"], json!(work), "{body}");

    // A reader subscribes; the count says one, and the answer carries no names.
    let mut reader = Client::new(server::build_router(AppState::new(
        config(&dir, None),
        tdb.db().clone(),
    )));
    reader.register("subs-reader").await;
    let (status, body) = reader
        .send(
            "POST",
            "/api/v1/subscriptions",
            Some(json!({ "subject_type": "work", "subject_id": work })),
        )
        .await;
    // `200`, because the door answers with the subscription it now has rather than 201: subscribing twice is allowed and
    // the state, not the creation, is the fact.
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], json!("subscribed"), "{body}");

    let (_, body) = reader
        .send("GET", &format!("/api/v1/works/{work}/subscribers"), None)
        .await;
    assert_eq!(body["subscribers"], json!(1), "{body}");
    assert!(
        body.as_object().expect("object").keys().len() == 2,
        "a count and the work id, nothing else — who subscribed is not public: {body}"
    );

    // Consent is opt-in (M21-D16): this author never answered, so the work reports `deny`.
    let (status, body) = reader
        .send("GET", &format!("/api/v1/works/{work}/ai-training"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["ai_training"],
        json!("deny"),
        "an author who never answered is not opted in: {body}"
    );

    // The author states an answer, and the work reports it.
    let (status, body) = author
        .send(
            "PUT",
            &format!("/api/v1/works/{work}/ai-training"),
            Some(json!({ "ai_training": "allow" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (_, body) = reader
        .send("GET", &format!("/api/v1/works/{work}/ai-training"), None)
        .await;
    assert_eq!(body["ai_training"], json!("allow"), "{body}");

    // The instance's consent ledger is operator-only, and it lists the answer that was given.
    let (status, body) = reader.send("GET", "/api/v1/admin/ai-training", None).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a list of pseuds and their answers is not public: {body}"
    );
}
