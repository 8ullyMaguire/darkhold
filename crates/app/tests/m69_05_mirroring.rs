//! M69-05 — mirroring doors: who may hold a copy, recording one, and the attribution a reader sees.
//!
//! The claims, in the order a reader meets them:
//!
//! * the eligible list is **computed** from published fingerprints, so a peer that is close appears with its number and a
//!   peer that enforces different rules appears with the rule that stopped it (not with a low score — a veto is not a score);
//! * recording a mirror to a peer that is not eligible is refused with the number, so "by taste similarity" is enforced at
//!   the door rather than being a sentence in a doc;
//! * the public attribution door prints the sentence a work page shows, names the peer, and says it is a mirror;
//! * while the switch is off every door is 404.

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
        "darkhold-m6905-{tag}-{}-{:?}",
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
            let Some((pair, _)) = text.split_once(';') else { continue };
            let Some((name, value)) = pair.split_once('=') else { continue };
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
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    async fn register(&mut self, handle: &str) -> String {
        let (status, body) = self
            .send(
                "POST",
                "/api/v1/auth/register",
                Some(json!({
                    "email": format!("m69-05-{handle}@example.test"),
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
    config.discovery.work_mirroring = open;
    // The fingerprint door that feeds this one is M69-02's, and it has its own switch.
    config.discovery.peering_suggestions = true;
    config.discovery.enforced_rules = vec!["csam".to_owned()];
    server::build_router(AppState::new(config, tdb.db().clone()))
}

fn peer_document(instance: &str, tags: &[(&str, f64)], enforced: &[&str]) -> String {
    let tags: Vec<Value> = tags
        .iter()
        .map(|(tag, weight)| json!({ "tag": tag, "weight": weight }))
        .collect();
    json!({
        "version": 1,
        "instance_id": instance,
        "generated_at": "2026-11-01T00:00:00Z",
        "tags": tags,
        "enforced": enforced,
    })
    .to_string()
}

/// This instance's own taste, written the way M69-01 reads it: a public `instance_themes` row.
async fn seed_own_taste(tdb: &TestDb, base_url: &str) {
    let vector = json!({ "angst": 0.5, "slowburn": 0.3, "fluff": 0.2 });
    darkhold_db::instance_theme::upsert_theme(tdb.db(), base_url, &vector, true, "2026-11-07T00:00:00Z")
        .await
        .expect("seed instance theme");
}

/// A published, public work, created through the only route that makes works.
async fn publish_a_work(client: &mut Client) -> String {
    let (status, body) = client
        .send("POST", "/api/v1/works", Some(json!({ "title": "The Storm" })))
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

#[tokio::test]
async fn a_neighbour_is_eligible_a_twin_is_vetoed_and_a_copy_is_recorded_with_its_attribution() {
    server::set_trust_proxy(false);
    let dir = scratch_dir("eligible");
    let tdb = TestDb::connect_with_dir("m69-05-eligible", &dir).await;
    let base_url = Config::development_defaults().site.base_url;
    seed_own_taste(&tdb, &base_url).await;

    let mut operator = Client::new(router(&tdb, &dir, None, true));
    let operator_id = operator.register("mirror-operator").await;
    operator.swap(router(&tdb, &dir, Some(&operator_id), true));

    // A neighbour with the same taste and the same declared layer.
    let (status, body) = operator
        .send(
            "PUT",
            "/api/v1/operator/peering/fingerprints",
            Some(json!({
                "document": peer_document(
                    "https://near.example",
                    &[("angst", 0.45), ("slowburn", 0.35), ("fluff", 0.20)],
                    &["csam"],
                ),
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // A near-ish neighbour: worth telling a reader about, not worth handing a copy of somebody's work to.
    let (status, body) = operator
        .send(
            "PUT",
            "/api/v1/operator/peering/fingerprints",
            Some(json!({
                "document": peer_document(
                    "https://near-ish.example",
                    &[("angst", 1.0), ("crack", 2.0)],
                    &["csam"],
                ),
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // A taste twin that enforces something else.
    let (status, body) = operator
        .send(
            "PUT",
            "/api/v1/operator/peering/fingerprints",
            Some(json!({
                "document": peer_document(
                    "https://twin.example",
                    &[("angst", 0.5), ("slowburn", 0.3), ("fluff", 0.2)],
                    &["csam", "no-archival"],
                ),
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = operator
        .send("GET", "/api/v1/operator/mirroring/eligible", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let eligible = body["eligible"].as_array().expect("a list");
    assert_eq!(eligible.len(), 1, "only the neighbour may hold a copy: {body}");
    assert_eq!(eligible[0]["instance_id"], "https://near.example");
    assert!(
        eligible[0]["similarity"].as_f64().unwrap_or(0.0) >= 0.5,
        "{body}"
    );
    let ineligible = body["ineligible"].as_array().expect("a list");
    assert_eq!(ineligible.len(), 2, "{body}");
    let twin = ineligible
        .iter()
        .find(|row| row["instance_id"] == "https://twin.example")
        .expect("the twin");
    assert!(
        twin["reason"]
            .as_str()
            .unwrap_or_default()
            .contains("no-archival"),
        "a veto names the rule, not a low score: {body}"
    );
    assert!(
        twin["similarity"].is_null(),
        "a vetoed peer has no similarity to report: {body}"
    );
    let near_ish = ineligible
        .iter()
        .find(|row| row["instance_id"] == "https://near-ish.example")
        .expect("the near-ish peer");
    assert!(
        near_ish["similarity"].as_f64().unwrap_or(0.0) > 0.25
            && near_ish["similarity"].as_f64().unwrap_or(1.0) < 0.5,
        "the mirror floor is stricter than the suggestion floor: {body}"
    );

    // Recording: refused for the twin, accepted for the neighbour.
    let work_id = publish_a_work(&mut operator).await;
    make_public(&tdb, &work_id).await;

    let (status, body) = operator
        .send(
            "POST",
            &format!("/api/v1/operator/mirroring/{work_id}/mirrors"),
            Some(json!({
                "peer_instance": "https://twin.example",
                "peer_url": "https://twin.example/works/storm",
            })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("no-archival"),
        "{body}"
    );

    // A peer the operator could not record even if they wanted to: it is above peering's floor and below the mirror floor,
    // and the refusal carries the number.
    let (status, body) = operator
        .send(
            "POST",
            &format!("/api/v1/operator/mirroring/{work_id}/mirrors"),
            Some(json!({
                "peer_instance": "https://near-ish.example",
                "peer_url": "https://near-ish.example/works/storm",
            })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("mirror floor"),
        "{body}"
    );

    let (status, body) = operator
        .send(
            "POST",
            &format!("/api/v1/operator/mirroring/{work_id}/mirrors"),
            Some(json!({
                "peer_instance": "https://near.example",
                "peer_url": "https://near.example/works/storm",
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["mirror"]["state"], "active");
    assert!(
        body["mirror"]["similarity_bp"].as_i64().unwrap_or(0) >= 5000,
        "the record carries the number the decision was made on: {body}"
    );
    let attribution = body["attribution"].as_str().expect("an attribution sentence");
    assert!(attribution.contains("https://near.example"), "{attribution}");
    assert!(attribution.contains("mirror"), "{attribution}");
    assert!(body["mirror"]["title"].is_string(), "{body}");

    // The work page's door, anonymous: no cookies at all.
    let mut stranger = Client::new(router(&tdb, &dir, Some(&operator_id), true));
    let (status, body) = stranger
        .send("GET", &format!("/api/v1/works/{work_id}/mirrors"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["active"], 1);
    let lines = body["attribution"].as_array().expect("lines");
    assert_eq!(lines.len(), 1);
    assert!(
        lines[0].as_str().unwrap_or_default().contains("preserved by https://near.example"),
        "{body}"
    );

    // A draft's mirrors are not a stranger's to see: 404, not an empty list. The operator creates a second work and leaves
    // it unpublished, which is what the creation route does by default.
    let hidden = publish_a_work(&mut operator).await;
    let (status, body) = stranger
        .send("GET", &format!("/api/v1/works/{hidden}/mirrors"), None)
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a draft answers 404 rather than disclosing that it exists: {body}"
    );
}

/// Make a work visible to strangers, which the creation route does not do on its own.
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
async fn with_the_switch_off_every_door_is_404() {
    server::set_trust_proxy(false);
    let dir = scratch_dir("off");
    let tdb = TestDb::connect_with_dir("m69-05-off", &dir).await;

    let mut client = Client::new(router(&tdb, &dir, None, false));
    let account = client.register("mirror-closed").await;
    client.swap(router(&tdb, &dir, Some(&account), false));

    for (method, path) in [
        ("GET", "/api/v1/operator/mirroring/eligible".to_owned()),
        ("POST", "/api/v1/operator/mirroring/w/mirrors".to_owned()),
        ("POST", "/api/v1/operator/mirroring/mirrors/i/retire".to_owned()),
        ("GET", "/api/v1/works/w/mirrors".to_owned()),
    ] {
        let (status, body) = client
            .send(method, &path, (method == "POST").then(|| json!({})))
            .await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "{method} {path} answered {status}: {body}"
        );
    }
}
