//! HTTP Signatures on `POST /federation/inbox` (ADR 0031, spec §25.1).
//!
//! The inbox used to accept anything. This file proves the four things that make "accepts anything" untrue, each with a real RSA
//! keypair — no mocked verifier, because a mocked verifier proves only that a function was called:
//!
//! * a **signed, fresh, digest-matching** request is accepted, stored, and its intake row says so with the key id;
//! * a request whose **body does not match the digest** is refused, and the refusal is recorded rather than merely returned;
//! * a **stale** date is refused: a signature with no freshness bound is a bearer token with no expiry;
//! * a signature that **does not cover** `(request-target) host date digest` is refused, because a signature over a subset is a
//!   signature over the parts a proxy can leave alone;
//! * an **unsigned** request is refused by default and recorded as `rejected`; an operator who turns the requirement off gets
//!   the old behaviour, with the row `quarantined` — the ledger still says nothing verified it.

use std::path::PathBuf;

use axum::body::Body;
use axum::http::{header, HeaderValue, Method, Request, StatusCode};
use axum::Router;
use base64::Engine as _;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::Database;
use rsa::pkcs1v15::SigningKey;
use rsa::pkcs8::EncodePublicKey;
use rsa::signature::{SignatureEncoding, Signer};
use rsa::{RsaPrivateKey, RsaPublicKey};
use serde_json::{json, Value};
use sha2::Sha256;
use test_support::TestDb;
use tower::ServiceExt;

const ACTOR: &str = "https://peer.example/users/archivist";

struct Peer {
    key: RsaPrivateKey,
    pem: String,
}

impl Peer {
    fn new() -> Self {
        let mut rng = rand::thread_rng();
        // 2048 bits: what a peer actually publishes. A 1024-bit key would make the test faster and prove less.
        let key = RsaPrivateKey::new(&mut rng, 2048).expect("an RSA key");
        let pem = RsaPublicKey::from(&key)
            .to_public_key_pem(rsa::pkcs8::LineEnding::LF)
            .expect("a PEM");
        Self { key, pem }
    }

    /// The `Signature` header for these exact headers, covering exactly `covered`.
    fn sign(&self, target: &str, headers: &[(&str, String)], covered: &[&str]) -> String {
        let mut lines = Vec::new();
        for name in covered {
            let value = if *name == "(request-target)" {
                format!("post {target}")
            } else {
                headers
                    .iter()
                    .find(|(key, _)| key.eq_ignore_ascii_case(name))
                    .map(|(_, value)| value.clone())
                    .unwrap_or_else(|| panic!("no {name} header to sign"))
            };
            lines.push(format!("{name}: {value}"));
        }
        let signing_key = SigningKey::<Sha256>::new(self.key.clone());
        let signature = signing_key.sign(lines.join("\n").as_bytes());
        let encoded = base64::engine::general_purpose::STANDARD.encode(signature.to_bytes());
        format!(
            r#"keyId="{ACTOR}#main-key",algorithm="rsa-sha256",headers="{}",signature="{encoded}""#,
            covered.join(" ")
        )
    }
}

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-sig-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn config(dir: &std::path::Path, require_signature: bool) -> Config {
    let mut config = Config::development_defaults();
    config.storage.root = dir.to_path_buf();
    config.federation.require_signature = require_signature;
    config
}

fn digest_of(body: &str) -> String {
    use sha2::Digest as _;
    format!(
        "SHA-256={}",
        base64::engine::general_purpose::STANDARD.encode(Sha256::digest(body.as_bytes()))
    )
}

fn date_now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc2822)
        .expect("a date")
}

/// Post `body` to the inbox with the given headers.
async fn post(
    app: Router,
    body: &str,
    headers: Vec<(&'static str, String)>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(Method::POST)
        .uri("/federation/inbox")
        .header(header::CONTENT_TYPE, "application/activity+json");
    for (name, value) in headers {
        builder = builder.header(name, HeaderValue::from_str(&value).expect("header value"));
    }
    let response = app
        .oneshot(builder.body(Body::from(body.to_owned())).expect("request"))
        .await
        .expect("response");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 65536)
        .await
        .expect("body");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// Every intake row, as `(state, note)`.
///
/// Ordered by nothing: `received_at` is a whole second and the id is a UUID, so "insertion order" is not a fact either engine
/// can report. A test that asserted on row *position* would pass on SQLite's rowid and fail on PostgreSQL, and would be
/// asserting on an implementation detail rather than on the ledger.
async fn ledger(db: &Database) -> Vec<(String, Option<String>)> {
    let sql = db.sql(
        "SELECT state, note FROM federation_inbound",
        "SELECT state, note FROM federation_inbound",
    );
    match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_as(&sql)
            .fetch_all(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("rows"),
        darkhold_db::Backend::Postgres => sqlx::query_as(&sql)
            .fetch_all(db.postgres_pool().expect("postgres"))
            .await
            .expect("rows"),
    }
}

fn activity() -> String {
    json!({
        "id": "https://peer.example/activities/1",
        "type": "Create",
        "actor": ACTOR,
        "object": "https://peer.example/objects/9",
    })
    .to_string()
}

#[tokio::test]
async fn a_signed_request_is_accepted_and_a_tampered_one_is_refused() {
    let dir = scratch_dir("accept");
    let tdb = TestDb::connect_with_dir("sigaccept", &dir).await;
    let state = AppState::new(config(&dir, true), tdb.db().clone());
    let peer = Peer::new();
    // Cache from the `keyId` the peer names in its header, which is the actor URL plus a fragment.
    state
        .peer_keys()
        .insert(&format!("{ACTOR}#main-key"), &peer.pem);
    let app = server::build_router(state.clone());
    let body = activity();

    // A well-formed signed request.
    let host = "localhost".to_owned();
    let date = date_now();
    let digest = digest_of(&body);
    let mut headers = vec![
        ("host", host.clone()),
        ("date", date.clone()),
        ("digest", digest.clone()),
    ];
    let covered = ["(request-target)", "host", "date", "digest"];
    let signature = peer.sign("/federation/inbox", &headers, &covered);
    headers.push(("signature", signature.clone()));

    let (status, json) = post(app.clone(), &body, headers.clone()).await;
    if status != StatusCode::OK {
        panic!(
            "signed request refused: {json} / ledger {:?}",
            ledger(tdb.db()).await
        );
    }

    let rows = ledger(tdb.db()).await;
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(
        rows[0].0, "accepted",
        "a verified request is accepted: {rows:?}"
    );
    assert!(
        rows[0].1.as_deref().unwrap_or_default().contains(ACTOR),
        "the note names the key that verified: {rows:?}"
    );

    // The same signature over a *different* body: the digest header still says the old hash, so it must not verify — and if the
    // digest header were updated too, the signature over it would no longer match. Either way the request is refused.
    let tampered = json!({
        "id": "https://peer.example/activities/2",
        "type": "Delete",
        "actor": ACTOR,
        "object": "https://peer.example/objects/9",
    })
    .to_string();
    let (status, _) = post(app.clone(), &tampered, headers.clone()).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "a body that does not match the digest must be refused"
    );
    let rows = ledger(tdb.db()).await;
    assert_eq!(rows.len(), 2, "the refusal is recorded: {rows:?}");
    assert_eq!(
        rows.iter().filter(|(state, _)| state == "rejected").count(),
        1,
        "exactly one row was refused: {rows:?}"
    );
    assert!(
        rows.iter().any(|(state, note)| state == "rejected"
            && note.as_deref().unwrap_or_default().contains("Digest")),
        "the refusal says what did not match: {rows:?}"
    );

    // A signature that covers only `host date` is refused by name.
    let weak = peer.sign(
        "/federation/inbox",
        &[("host", host.clone()), ("date", date.clone())],
        &["host", "date"],
    );
    let mut weak_headers = headers.clone();
    weak_headers.retain(|(name, _)| *name != "signature");
    weak_headers.push(("signature", weak));
    let (status, _) = post(app.clone(), &body, weak_headers).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "a signature that does not cover (request-target) and digest is refused"
    );

    // A stale date is refused, even with a signature that is otherwise perfect.
    let stale = time::OffsetDateTime::now_utc() - time::Duration::hours(2);
    let stale_date = stale
        .format(&time::format_description::well_known::Rfc2822)
        .expect("a date");
    let mut stale_headers = vec![
        ("host", host.clone()),
        ("date", stale_date.clone()),
        ("digest", digest.clone()),
    ];
    let stale_signature = peer.sign("/federation/inbox", &stale_headers, &covered);
    stale_headers.push(("signature", stale_signature));
    let (status, _) = post(app.clone(), &body, stale_headers).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "a stale date is refused");

    // A key this instance does not hold, and cannot fetch (the host does not resolve), is a refusal rather than a panic.
    let stranger = Peer::new();
    let mut stranger_headers = vec![
        ("host", host.clone()),
        ("date", date_now()),
        ("digest", digest.clone()),
    ];
    let stranger_signature = stranger.sign("/federation/inbox", &stranger_headers, &covered);
    stranger_headers.push(("signature", stranger_signature));
    let (status, _) = post(app, &body, stranger_headers).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "an unknown key is refused, not trusted"
    );
}

#[tokio::test]
async fn an_unsigned_request_is_refused_by_default_and_quarantined_when_the_operator_allows_it() {
    let body = activity();

    // Default configuration: refused.
    let dir = scratch_dir("strict");
    let tdb = TestDb::connect_with_dir("sigstrict", &dir).await;
    let state = AppState::new(config(&dir, true), tdb.db().clone());
    let app = server::build_router(state);
    let (status, json) = post(app, &body, Vec::new()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{json}");
    let rows = ledger(tdb.db()).await;
    assert_eq!(rows.len(), 1, "the refusal is an intake row: {rows:?}");
    assert_eq!(rows[0].0, "rejected", "{rows:?}");
    assert!(
        rows[0]
            .1
            .as_deref()
            .unwrap_or_default()
            .contains("Signature"),
        "the note says what was missing: {rows:?}"
    );

    // An operator who allows unsigned traffic: accepted, but the row says nothing verified it.
    let dir = scratch_dir("open");
    let tdb = TestDb::connect_with_dir("sigopen", &dir).await;
    let state = AppState::new(config(&dir, false), tdb.db().clone());
    let app = server::build_router(state);
    let (status, json) = post(app, &body, Vec::new()).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let rows = ledger(tdb.db()).await;
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(
        rows[0].0, "quarantined",
        "an unsigned row waits for the operator, it is not accepted: {rows:?}"
    );
}
