//! The federation intake ledger (§25.1) — what arrives, and the operator's answer to it.
//!
//! `POST /federation/inbox` has always stored the activity and told the peer "received". Nothing recorded that the request was
//! **unverified**, and `federation_inbound` — the table whose state column says `accepted|quarantined|rejected` — had one writer
//! no caller and no reader at all.
//!
//! These are the properties only the route can have:
//!
//! * an activity that arrives is entered in the ledger as `quarantined`, with the peer's host taken from the actor id;
//! * an actor id that is not a URL records `unknown` rather than dropping the evidence;
//! * the ledger is read by the operator and by nobody else — a caller without operator standing gets `404` (ADR 0030);
//! * a decision moves the row to `accepted` or `rejected`, records the note, and cannot put it **back** in quarantine;
//! * an unknown state and an unknown id are different refusals with different codes.

use std::path::PathBuf;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::Database;
use serde_json::{json, Value};
use test_support::TestDb;
use tower::ServiceExt;

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-intake-{tag}-{}-{:?}",
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

    async fn send(&mut self, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut builder = Request::builder()
            .method(Method::from_bytes(method.as_bytes()).expect("method"))
            .uri(uri);
        if !self.cookies.is_empty() {
            let cookie = self
                .cookies
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect::<Vec<String>>()
                .join("; ");
            builder = builder.header(header::COOKIE, cookie);
        }
        if let Some((_, token)) = self.cookies.iter().find(|(k, _)| k == "darkhold_csrf") {
            builder = builder.header("x-csrf-token", token.clone());
        }
        let body = match body {
            Some(value) => {
                builder = builder.header(header::CONTENT_TYPE, "application/json");
                Body::from(value.to_string())
            }
            None => Body::empty(),
        };
        let response = self
            .app
            .clone()
            .oneshot(builder.body(body).expect("request"))
            .await
            .expect("response");
        let status = response.status();
        for value in response.headers().get_all(header::SET_COOKIE) {
            let Ok(raw) = value.to_str() else { continue };
            let pair = raw.split(';').next().unwrap_or(raw);
            let Some((name, val)) = pair.split_once('=') else {
                continue;
            };
            self.cookies.retain(|(key, _)| key != name);
            if !val.is_empty() {
                self.cookies.push((name.to_owned(), val.to_owned()));
            }
        }
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
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
                    "email": format!("intake-{handle}@example.test"),
                    "password": "test-pass-1234",
                    "handle": handle,
                    "display_name": handle,
                    "age_band": "adult",
                })),
            )
            .await;
        assert!(status.is_success(), "register {handle}: {body}");
        let (_, me) = self.send("GET", "/api/v1/auth/me", None).await;
        me["account"]["id"]
            .as_str()
            .or_else(|| me["id"].as_str())
            .expect("account id")
            .to_owned()
    }
}

fn base_config(dir: &std::path::Path) -> Config {
    let mut config = Config::development_defaults();
    config.storage.root = dir.to_path_buf();
    // This file is about the *ledger*, not the signature: unsigned traffic is allowed here so an arrival still reaches the
    // table. `m70_13_inbox_signature` is where the default refusal is proven.
    config.federation.require_signature = false;
    config
}

#[tokio::test]
async fn an_arriving_activity_is_quarantined_and_only_the_operator_decides() {
    let dir = scratch_dir("decide");
    let tdb = TestDb::connect_with_dir("intake", &dir).await;
    let db: Database = tdb.db().clone();

    let config = base_config(&dir);
    let mut anonymous = Client::new(server::build_router(AppState::new(
        config.clone(),
        db.clone(),
    )));

    // The federation router is merged at the root, not under `/api/v1`: an ActivityPub inbox lives at the address other
    // servers were told about, and `/api/v1/federation/inbox` has never been it.
    //
    // A peer posts an activity. There is no authentication on an inbox — that is the point of a quarantine ledger.
    let (status, body) = anonymous
        .send(
            "POST",
            "/federation/inbox",
            Some(json!({
                "id": "https://peer.example/activities/1",
                "type": "Create",
                "actor": "https://peer.example/users/archivist",
                "object": "https://peer.example/objects/9",
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // A second one with an actor that is not a URL: the ledger still gets a row, and says `unknown`.
    let (status, _) = anonymous
        .send(
            "POST",
            "/federation/inbox",
            Some(json!({ "type": "Announce", "actor": "", "object": "x" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    // Registering happens after the posts: the inbox is public, the ledger is not.
    let operator_account = anonymous.register("fedoperator").await;
    let mut operator_config = config.clone();
    operator_config.administration.operator_account_id =
        Some(operator_account.parse().expect("account id"));
    let mut operator = Client::new(server::build_router(AppState::new(
        operator_config,
        db.clone(),
    )));
    operator.cookies = anonymous.cookies.clone();

    let (status, body) = operator
        .send("GET", "/operator/federation/inbound", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let records = body["records"].as_array().expect("records");
    assert_eq!(records.len(), 2, "{body}");
    assert_eq!(body["quarantined_in_page"], json!(2), "{body}");

    let create = records
        .iter()
        .find(|row| row["object_type"] == json!("Create"))
        .expect("the Create row");
    assert_eq!(create["state"], json!("quarantined"), "{create}");
    assert_eq!(
        create["peer_host"],
        json!("peer.example"),
        "the host comes from the actor id: {create}"
    );
    assert_eq!(
        create["object_id"],
        json!("https://peer.example/objects/9"),
        "the object is what arrived, not the actor: {create}"
    );

    let unknown = records
        .iter()
        .find(|row| row["object_type"] == json!("Announce"))
        .expect("the Announce row");
    assert_eq!(
        unknown["peer_host"],
        json!("unknown"),
        "an actor that is not a URL is recorded, not dropped: {unknown}"
    );

    // A reader who is not the operator cannot see the ledger, and the refusal does not confirm it exists.
    let mut reader = Client::new(server::build_router(AppState::new(
        config.clone(),
        db.clone(),
    )));
    reader.register("justareader").await;
    let (status, body) = reader
        .send("GET", "/operator/federation/inbound", None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // The operator accepts one, with a note.
    let id = create["id"].as_str().expect("id").to_owned();
    let (status, body) = operator
        .send(
            "POST",
            &format!("/operator/federation/inbound/{id}/state"),
            Some(json!({ "state": "accepted", "note": "known peer, signed" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["state"], json!("accepted"), "{body}");

    // The decision is in the row, and the filter finds it by the new state.
    let (_, body) = operator
        .send("GET", "/operator/federation/inbound?state=accepted", None)
        .await;
    let accepted = body["records"].as_array().expect("records");
    assert_eq!(accepted.len(), 1, "{body}");
    assert_eq!(accepted[0]["note"], json!("known peer, signed"), "{body}");

    // `quarantined` is where intake starts and not a decision: it is refused as a target state.
    let (status, body) = operator
        .send(
            "POST",
            &format!("/operator/federation/inbound/{id}/state"),
            Some(json!({ "state": "quarantined" })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    // An unknown state is a 422; an unknown id is a 404. Different questions, different answers.
    let (status, _) = operator
        .send(
            "POST",
            &format!("/operator/federation/inbound/{id}/state"),
            Some(json!({ "state": "maybe" })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = operator
        .send(
            "POST",
            "/operator/federation/inbound/00000000-0000-0000-0000-000000000000/state",
            Some(json!({ "state": "rejected" })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // And the filter refuses a state that is not in the vocabulary rather than returning everything.
    let (status, body) = operator
        .send("GET", "/operator/federation/inbound?state=pending", None)
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
}
