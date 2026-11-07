//! M69-07 — the evacuation doors.
//!
//! What a reader of this test should be able to conclude:
//!
//! * announcing computes the recipient list from published fingerprints — an operator does not type who takes what;
//! * the plan spreads the works instead of pouring them all into the closest peer;
//! * settling one placement names the home, and a later evacuation cannot move that work to another peer;
//! * the receipt the operator reads says who took what, and no fork exists;
//! * with the switch off, every door is 404.

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
        "darkhold-m6907-{tag}-{}-{:?}",
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

    fn swap(&mut self, app: axum::Router) {
        self.app = app;
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
                    "email": format!("m69-07-{handle}@example.test"),
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
        let (_, me) = self.send("GET", "/api/v1/auth/me", None).await;
        me["account"]["id"]
            .as_str()
            .or_else(|| me["id"].as_str())
            .expect("account id")
            .to_owned()
    }
}

fn router(tdb: &TestDb, dir: &std::path::Path, operator: Option<&str>, open: bool) -> axum::Router {
    let mut config = Config::development_defaults();
    config.storage.root = dir.to_path_buf();
    config.administration.operator_account_id = operator.map(|id| id.parse().expect("account id"));
    config.discovery.evacuation = open;
    config.discovery.peering_suggestions = true;
    config.discovery.enforced_rules = vec!["csam".to_owned()];
    server::build_router(AppState::new(config, tdb.db().clone()))
}

fn peer_document(instance: &str, tags: &[(&str, f64)]) -> String {
    let tags: Vec<Value> = tags
        .iter()
        .map(|(tag, weight)| json!({ "tag": tag, "weight": weight }))
        .collect();
    json!({
        "version": 1,
        "instance_id": instance,
        "generated_at": "2026-11-01T00:00:00Z",
        "tags": tags,
        "enforced": ["csam"],
    })
    .to_string()
}

async fn seed_own_taste(tdb: &TestDb, base_url: &str) {
    let vector = json!({ "angst": 0.5, "slowburn": 0.3, "fluff": 0.2 });
    darkhold_db::instance_theme::upsert_theme(
        tdb.db(),
        base_url,
        &vector,
        true,
        "2026-11-07T00:00:00Z",
    )
    .await
    .expect("seed instance theme");
}

/// A published, public work, created through the only route that makes works.
async fn publish_a_work(client: &mut Client, title: &str) -> String {
    let (status, body) = client
        .send("POST", "/api/v1/works", Some(json!({ "title": title })))
        .await;
    assert!(
        status == StatusCode::CREATED || status == StatusCode::OK,
        "create work: {body}"
    );
    let id = body["work"]["id"]
        .as_str()
        .or_else(|| body["id"].as_str())
        .expect("a work id")
        .to_owned();
    id
}

async fn make_public(tdb: &TestDb, work_id: &str) {
    let sql = tdb.db().sql(
        "UPDATE works SET lifecycle = 'published', visibility = 'public' WHERE id = ?",
        "UPDATE works SET lifecycle = 'published', visibility = 'public' WHERE id::text = $1",
    );
    match tdb.db().backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(work_id)
                .execute(tdb.db().sqlite_pool().expect("sqlite"))
                .await
                .expect("publish");
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(&sql)
                .bind(work_id)
                .execute(tdb.db().postgres_pool().expect("postgres"))
                .await
                .expect("publish");
        }
    }
}

#[tokio::test]
async fn the_plan_spreads_the_works_and_the_receipt_names_who_took_what() {
    server::set_trust_proxy(false);
    let dir = scratch_dir("go");
    let tdb = TestDb::connect_with_dir("m69-07-go", &dir).await;
    let base_url = Config::development_defaults().site.base_url;
    seed_own_taste(&tdb, &base_url).await;

    let mut operator = Client::new(router(&tdb, &dir, None, true));
    let operator_id = operator.register("evac-operator").await;
    operator.swap(router(&tdb, &dir, Some(operator_id.as_str()), true));

    // Two peers close enough to receive, one too far.
    for (instance, tags) in [
        (
            "https://a.example",
            vec![("angst", 0.45), ("slowburn", 0.35), ("fluff", 0.20)],
        ),
        (
            "https://b.example",
            vec![("angst", 0.50), ("slowburn", 0.30), ("fluff", 0.20)],
        ),
        ("https://far.example", vec![("crack", 1.0)]),
    ] {
        let (status, body) = operator
            .send(
                "PUT",
                "/api/v1/operator/peering/fingerprints",
                Some(json!({ "document": peer_document(instance, &tags) })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    // Four works to evacuate.
    let mut work_ids = Vec::new();
    for index in 0..4 {
        let id = publish_a_work(&mut operator, &format!("Work {index}")).await;
        make_public(&tdb, &id).await;
        work_ids.push(id);
    }

    let (status, body) = operator
        .send(
            "POST",
            "/api/v1/operator/evacuation",
            Some(json!({ "reason": "the host is being retired" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["plan"]["placed"], 4, "{body}");
    let notice_id = body["notice"]["id"]
        .as_str()
        .expect("a notice id")
        .to_owned();
    let load = body["plan"]["load"].as_array().expect("load");
    assert_eq!(
        load.len(),
        2,
        "two close peers, and the far one takes none: {body}"
    );
    assert!(
        load.iter().all(|row| row["works"].as_i64() == Some(2)),
        "the works are spread, not poured into the closest peer: {body}"
    );
    assert!(
        !body["plan"]["load"]
            .as_array()
            .expect("load")
            .iter()
            .any(|row| row["recipient"] == "https://far.example"),
        "{body}"
    );

    let assignments = body["assignments"].as_array().expect("assignments").clone();
    let first = assignments[0].clone();
    let assignment_id = first["id"].as_str().expect("id").to_owned();
    let recipient = first["recipient"].as_str().expect("recipient").to_owned();
    let work_id = first["work_id"].as_str().expect("work").to_owned();

    // A second announcement while one is open is refused.
    let (status, body) = operator
        .send(
            "POST",
            "/api/v1/operator/evacuation",
            Some(json!({ "reason": "again" })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("already announced"),
        "{body}"
    );

    // Settle one, then read the receipt.
    let (status, body) = operator
        .send(
            "POST",
            &format!("/api/v1/operator/evacuation/assignments/{assignment_id}/settle"),
            Some(json!({ "state": "accepted", "note": "confirmed by mail" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["assignment"]["state"], "accepted");

    let (status, body) = operator
        .send("GET", "/api/v1/operator/evacuation", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["offered"], 3, "{body}");
    assert_eq!(body["receipt"]["accepted"], 1, "{body}");
    assert_eq!(
        body["receipt"]["taken"][0]["recipient"], recipient,
        "the receipt names who took what: {body}"
    );
    assert_eq!(body["receipt"]["forks"].as_array().expect("forks").len(), 0);

    // Close it: the receipt survives, and the notice is no longer open.
    let (status, body) = operator
        .send(
            "POST",
            "/api/v1/operator/evacuation/close",
            Some(json!({ "state": "completed" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["notice"]["state"], "completed");
    let (status, body) = operator
        .send("GET", "/api/v1/operator/evacuation", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["notice"].is_null(), "nothing is announced: {body}");

    // The receipt is still readable after the close, which is what an author asking "where did my work go?" needs.
    let stored = darkhold_db::evacuation::receipt(tdb.db(), &notice_id)
        .await
        .expect("the receipt outlives the open state");
    assert_eq!(stored.accepted.len(), 1);
    assert_eq!(stored.accepted[0].work_id, work_id);
    assert!(stored.forks().is_empty());
    let notice = darkhold_db::evacuation::find_notice(tdb.db(), &notice_id)
        .await
        .expect("find")
        .expect("the notice stays");
    assert_eq!(notice.state, "completed");
}

#[tokio::test]
async fn with_no_eligible_peer_an_evacuation_is_refused_rather_than_planned_empty() {
    server::set_trust_proxy(false);
    let dir = scratch_dir("nopeers");
    let tdb = TestDb::connect_with_dir("m69-07-nopeers", &dir).await;
    let base_url = Config::development_defaults().site.base_url;
    seed_own_taste(&tdb, &base_url).await;

    let mut operator = Client::new(router(&tdb, &dir, None, true));
    let operator_id = operator.register("evac-operator2").await;
    operator.swap(router(&tdb, &dir, Some(operator_id.as_str()), true));

    let (status, body) = operator
        .send(
            "POST",
            "/api/v1/operator/evacuation",
            Some(json!({ "reason": "the host is being retired" })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("no peer is eligible"),
        "a refusal, because a receipt of zero looks like the work was placed: {body}"
    );
}

#[tokio::test]
async fn with_the_switch_off_every_door_is_404() {
    server::set_trust_proxy(false);
    let dir = scratch_dir("off");
    let tdb = TestDb::connect_with_dir("m69-07-off", &dir).await;
    let mut client = Client::new(router(&tdb, &dir, None, false));
    let account = client.register("evac-closed").await;
    client.swap(router(&tdb, &dir, Some(account.as_str()), false));

    for (method, path) in [
        ("GET", "/api/v1/operator/evacuation"),
        ("POST", "/api/v1/operator/evacuation"),
        ("POST", "/api/v1/operator/evacuation/close"),
        ("POST", "/api/v1/operator/evacuation/assignments/x/settle"),
    ] {
        let (status, body) = client
            .send(method, path, (method == "POST").then(|| json!({})))
            .await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "{method} {path} answered {status}: {body}"
        );
    }
}
