//! M16-14 — webhook operations end to end: rotation, the rate limit, the breaker.
//!
//! The arithmetic (grace window, `RateLimit::allows`, `Breaker::after_failure`) has unit tests in
//! `darkhold-domain`. This file proves the store and the doors hold it: a rotation that keeps the old
//! secret admissible, a delivery count that ignores its own throttled rows, an endpoint that switches
//! itself off at the threshold and stays off until somebody says otherwise.

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
        "darkhold-m1614-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

struct Harness {
    _dir: PathBuf,
    tdb: TestDb,
    config: Config,
}

impl Harness {
    async fn new(tag: &str) -> Self {
        server::set_trust_proxy(false);
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
                    "email": format!("m16-14-{handle}@example.test"),
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
        let (status, me) = self.send("GET", "/api/v1/auth/me", None).await;
        assert_eq!(status, StatusCode::OK, "{me}");
        me["pseuds"][0]["id"].as_str().expect("pseud id").to_owned()
    }
}

fn message(body: &Value) -> String {
    body["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

#[tokio::test]
async fn rotating_a_secret_returns_the_new_one_once_and_keeps_the_old_one_working() {
    let h = Harness::new("rotate").await;
    let mut owner = h.client();
    owner.register("hook-owner").await;

    let (status, created) = owner
        .send(
            "POST",
            "/api/v1/me/webhooks",
            Some(json!({"url": "https://example.com/hook", "events": ["work.published"]})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let id = created["id"].as_str().expect("id").to_owned();
    let first = created["secret"].as_str().expect("secret").to_owned();
    assert!(first.starts_with("whsec_"), "{first}");

    let (status, rotated) = owner
        .send(
            "POST",
            &format!("/api/v1/me/webhooks/{id}/rotate"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{rotated}");
    let second = rotated["secret"].as_str().expect("new secret").to_owned();
    assert_ne!(
        second, first,
        "a rotation that returns the old secret is not a rotation"
    );
    assert_eq!(
        rotated["previous_valid_for_seconds"],
        json!(darkhold_domain::webhook::ROTATION_GRACE_SECONDS),
        "the response must say how long the old secret keeps verifying: {rotated}"
    );

    // The delivery projection carries both keys during the window, which is what lets `send_with_keys`
    // sign with `v1` and `v0` without re-reading the row.
    let rows = darkhold_db::marketplace::list_all_active_webhooks(h.tdb.db())
        .await
        .expect("list webhooks");
    let row = rows
        .iter()
        .find(|r| r["id"] == json!(id))
        .expect("endpoint row");
    assert_eq!(row["secret"], json!(second));
    assert_eq!(
        row["previous_secret"],
        json!(first),
        "the replaced secret stays admissible for the grace window: {row}"
    );
    let rotated_at = row["rotated_at"].as_str().expect("rotated_at");
    assert!(
        !rotated_at.is_empty(),
        "a grace without a start time never ends: {row}"
    );

    // A second rotation makes the *first* new secret the previous one: there is only ever one previous.
    let (status, third) = owner
        .send(
            "POST",
            &format!("/api/v1/me/webhooks/{id}/rotate"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{third}");
    let rows = darkhold_db::marketplace::list_all_active_webhooks(h.tdb.db())
        .await
        .expect("list webhooks");
    let row = rows
        .iter()
        .find(|r| r["id"] == json!(id))
        .expect("endpoint row");
    assert_eq!(
        row["previous_secret"],
        json!(second),
        "only one key is on the way out"
    );
}

#[tokio::test]
async fn a_rotation_by_a_stranger_is_the_same_answer_as_a_missing_subscription() {
    let h = Harness::new("stranger").await;
    let mut owner = h.client();
    owner.register("hook-owner").await;
    let (_, created) = owner
        .send(
            "POST",
            "/api/v1/me/webhooks",
            Some(json!({"url": "https://example.com/hook", "events": ["*"]})),
        )
        .await;
    let id = created["id"].as_str().expect("id").to_owned();

    let mut stranger = h.client();
    stranger.register("not-the-owner").await;
    let (status, body) = stranger
        .send(
            "POST",
            &format!("/api/v1/me/webhooks/{id}/rotate"),
            Some(json!({})),
        )
        .await;
    assert!(
        status == StatusCode::UNPROCESSABLE_ENTITY || status == StatusCode::NOT_FOUND,
        "a stranger must not be able to rotate somebody else's secret: {status} {body}"
    );
    assert!(
        body["error"].is_object(),
        "the refusal names no secret and no owner: {body}"
    );

    // And the owner's secret is unchanged.
    let rows = darkhold_db::marketplace::list_all_active_webhooks(h.tdb.db())
        .await
        .expect("list");
    let row = rows.iter().find(|r| r["id"] == json!(id)).expect("row");
    assert_eq!(
        row["secret"], created["secret"],
        "the stranger changed nothing"
    );
}

#[tokio::test]
async fn consecutive_failures_switch_an_endpoint_off_and_only_an_explicit_enable_brings_it_back() {
    let h = Harness::new("breaker").await;
    let db = h.tdb.db();
    let id = darkhold_db::marketplace::create_webhook(
        db,
        "owner-1",
        "https://example.com/dead",
        "whsec_breaker",
        &["work.published".to_string()],
    )
    .await
    .expect("create webhook");

    let threshold = darkhold_domain::webhook::DEFAULT_FAILURE_THRESHOLD;
    for attempt in 1..threshold {
        let reason =
            darkhold_db::webhooks::record_failure(db, &id, threshold, "2026-11-07T00:00:00Z")
                .await
                .expect("record failure");
        assert!(
            reason.is_none(),
            "failure {attempt} is below the threshold: {reason:?}"
        );
    }

    let subs = darkhold_db::webhooks::subscriptions(db)
        .await
        .expect("subscriptions");
    let row = subs.iter().find(|r| r["id"] == json!(id)).expect("row");
    assert_eq!(
        row["active"],
        json!(true),
        "still active one failure short: {row}"
    );
    assert_eq!(row["consecutive_failures"], json!(i64::from(threshold - 1)));

    // The failure that crosses the line.
    let reason = darkhold_db::webhooks::record_failure(db, &id, threshold, "2026-11-07T00:01:00Z")
        .await
        .expect("record failure")
        .expect("the threshold switches it off");
    assert!(reason.contains("consecutive"), "{reason}");

    let subs = darkhold_db::webhooks::subscriptions(db)
        .await
        .expect("subscriptions");
    let row = subs.iter().find(|r| r["id"] == json!(id)).expect("row");
    assert_eq!(row["active"], json!(false), "{row}");
    assert_eq!(row["disabled_reason"], json!(reason), "{row}");
    assert_eq!(row["disabled_at"], json!("2026-11-07T00:01:00Z"));

    // A disabled endpoint is not in the delivery list, so it receives nothing and cannot fail again.
    let deliverable = darkhold_db::marketplace::list_all_active_webhooks(db)
        .await
        .expect("active webhooks");
    assert!(
        !deliverable.iter().any(|r| r["id"] == json!(id)),
        "a switched-off endpoint must not be delivered to"
    );

    // A success while disabled does not resurrect it: only an explicit enable does.
    darkhold_db::webhooks::record_success(db, &id)
        .await
        .expect("success");
    let subs = darkhold_db::webhooks::subscriptions(db)
        .await
        .expect("subscriptions");
    let row = subs.iter().find(|r| r["id"] == json!(id)).expect("row");
    assert_eq!(
        row["active"],
        json!(false),
        "the breaker needs a human: {row}"
    );
    assert_eq!(
        row["consecutive_failures"],
        json!(0),
        "the count reset, the switch did not"
    );

    // And the explicit enable clears both the flag and the reason.
    assert!(darkhold_db::webhooks::reenable(db, &id, Some("owner-1"))
        .await
        .expect("reenable"));
    let subs = darkhold_db::webhooks::subscriptions(db)
        .await
        .expect("subscriptions");
    let row = subs.iter().find(|r| r["id"] == json!(id)).expect("row");
    assert_eq!(row["active"], json!(true), "{row}");
    assert_eq!(
        row["disabled_reason"],
        Value::Null,
        "a stale reason on an active endpoint reads as a fault: {row}"
    );

    // A different owner cannot re-enable it.
    darkhold_db::webhooks::record_failure(db, &id, 1, "2026-11-07T00:02:00Z")
        .await
        .expect("fail again");
    assert!(
        !darkhold_db::webhooks::reenable(db, &id, Some("somebody-else"))
            .await
            .expect("reenable attempt"),
        "ownership is checked, not assumed"
    );
}

#[tokio::test]
async fn the_rate_limit_counts_deliveries_not_attempts_it_never_made() {
    let h = Harness::new("ratelimit").await;
    let db = h.tdb.db();
    let id = darkhold_db::marketplace::create_webhook(
        db,
        "owner-2",
        "https://example.com/busy",
        "whsec_busy",
        &["work.published".to_string()],
    )
    .await
    .expect("create webhook");

    for n in 0..3 {
        darkhold_db::marketplace::record_delivery(db, &id, &format!("evt-{n}"), "{}", "sig", "ok")
            .await
            .expect("record delivery");
    }
    let since = "2000-01-01T00:00:00Z";
    let counted = darkhold_db::webhooks::delivered_since(db, &id, since)
        .await
        .expect("count");
    assert_eq!(counted, 3);

    // A throttled row is a delivery that never happened. Counting it would let the limiter consume its own
    // budget, so a burst would extend its own silence.
    darkhold_db::marketplace::record_delivery(db, &id, "evt-throttled", "{}", "", "throttled")
        .await
        .expect("record throttled");
    let counted = darkhold_db::webhooks::delivered_since(db, &id, since)
        .await
        .expect("count");
    assert_eq!(counted, 3, "throttled rows do not count against the limit");

    // The limit itself is the domain's answer, read against that count.
    let limit = darkhold_domain::webhook::RateLimit { per_minute: 3 };
    assert!(!limit.allows(u64::try_from(counted).unwrap_or(0)));
    assert!(darkhold_domain::webhook::RateLimit { per_minute: 4 }
        .allows(u64::try_from(counted).unwrap_or(0)));

    // And the window is the window: rows older than `since` are not counted.
    let counted = darkhold_db::webhooks::delivered_since(db, &id, "2999-01-01T00:00:00Z")
        .await
        .expect("count");
    assert_eq!(counted, 0);
}

#[tokio::test]
async fn only_an_operator_reads_the_subscription_health_and_no_secret_is_in_it() {
    let mut h = Harness::new("health").await;
    let mut owner = h.client();
    owner.register("hook-owner").await;
    let (_, created) = owner
        .send(
            "POST",
            "/api/v1/me/webhooks",
            Some(json!({"url": "https://example.com/hook", "events": ["*"]})),
        )
        .await;
    let id = created["id"].as_str().expect("id").to_owned();

    // A signed-in reader who is not an operator. `404` and not `403` (ADR 0030): the operator surface's existence is the
    // disclosure, and a webhook console is a surface.
    let (status, body) = owner.send("GET", "/api/v1/operator/webhooks", None).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "403 would confirm the webhook console exists: {body}"
    );

    // Now the same account as the configured operator.
    let (_, me) = owner.send("GET", "/api/v1/auth/me", None).await;
    let account = me["account"]["id"]
        .as_str()
        .or_else(|| me["id"].as_str())
        .expect("account id")
        .to_owned();
    h.config.administration.operator_account_id = Some(account.parse().expect("account id"));

    let mut operator = h.client();
    // The operator is the same person: carry the session cookies over rather than registering again.
    operator.cookies = owner.cookies.clone();

    let (status, body) = operator
        .send("GET", "/api/v1/operator/webhooks", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let subs = body["subscriptions"].as_array().expect("subscriptions");
    let row = subs.iter().find(|r| r["id"] == json!(id)).expect("row");
    assert!(
        row.get("secret").is_none(),
        "an operator must not be able to forge a subscriber's deliveries: {row}"
    );
    assert_eq!(
        row["rate_per_minute"],
        json!(60),
        "the default limit is visible: {row}"
    );
    assert_eq!(row["consecutive_failures"], json!(0));

    // Re-enabling through the door is the operator's act, and it reports what it did.
    let (status, body) = operator
        .send(
            "POST",
            &format!("/api/v1/operator/webhooks/{id}/enable"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["active"], json!(true), "{body}");

    let (status, body) = operator
        .send(
            "POST",
            "/api/v1/operator/webhooks/no-such-endpoint/enable",
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(message(&body).contains("no subscription"), "{body}");
}

// ---------------------------------------------------------------------------
// The wiring: `deliver_notification` is where the three gates actually run.
// ---------------------------------------------------------------------------
//
// A unit test proves `RateLimit::allows` answers correctly; it does not prove the delivery loop asks.
// These two tests run the real loop against a local receiver, so "the limit is enforced" and "the
// breaker switches the endpoint off" are statements about the code that sends, not about the code that
// computes.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use darkhold_db::outbox::OutboxEvent;

/// A tiny HTTP receiver: counts requests and answers with `status`, one connection at a time.
async fn receiver(status: &'static str) -> (u16, Arc<AtomicUsize>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&hits);
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let counter = Arc::clone(&counter);
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = [0u8; 8192];
                // Read the request head; the body is irrelevant to the count.
                let _ = socket.read(&mut buf).await;
                counter.fetch_add(1, Ordering::SeqCst);
                let body =
                    format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                let _ = socket.write_all(body.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    (port, hits)
}

fn event(id: &str) -> OutboxEvent {
    OutboxEvent {
        id: id.to_owned(),
        topic: "publish.notify".to_owned(),
        payload: json!({
            "event_type": "work.published",
            "created_at": "2026-11-07T00:00:00Z",
            "work_id": "w-1",
        })
        .to_string(),
        attempts: 0,
        created_at: "2026-11-07T00:00:00Z".to_owned(),
    }
}

#[tokio::test]
async fn the_delivery_loop_throttles_a_subscription_at_its_own_limit_and_records_the_skip() {
    let mut h = Harness::new("loop-limit").await;
    h.config.administration.webhook_allowed_hosts = vec!["127.0.0.1".to_owned()];
    let (port, hits) = receiver("200 OK").await;

    let db = h.tdb.db();
    let id = darkhold_db::marketplace::create_webhook(
        db,
        "owner-loop",
        &format!("http://127.0.0.1:{port}/hook"),
        "whsec_loop",
        &["*".to_string()],
    )
    .await
    .expect("create webhook");
    // One delivery a minute, so the second call in this test cannot fit in the window.
    test_support::exec(
        db,
        "UPDATE webhook_endpoints SET rate_per_minute = 1 WHERE id = ?",
        "UPDATE webhook_endpoints SET rate_per_minute = 1 WHERE id = $1",
        &id,
    )
    .await;

    let state = AppState::new(h.config.clone(), db.clone());
    darkhold_app::webhook_delivery::deliver_notification(&state, &event("evt-a"))
        .await
        .expect("deliver first");
    darkhold_app::webhook_delivery::deliver_notification(&state, &event("evt-b"))
        .await
        .expect("deliver second");

    // Give the receiver task a moment to finish counting the first request.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(
        hits.load(Ordering::SeqCst),
        1,
        "the second delivery is over the subscription's own limit"
    );

    let throttled = test_support::scalar_i64(
        db,
        "SELECT COUNT(*) FROM webhook_deliveries WHERE endpoint_id = ? AND status = 'throttled'",
        "SELECT COUNT(*) FROM webhook_deliveries WHERE endpoint_id = $1 AND status = 'throttled'",
        &id,
    )
    .await;
    assert_eq!(
        throttled, 1,
        "a delivery that was never attempted is recorded, because its absence looks like an outage"
    );
}

#[tokio::test]
async fn the_delivery_loop_switches_off_an_endpoint_whose_receiver_keeps_failing() {
    let mut h = Harness::new("loop-breaker").await;
    h.config.administration.webhook_allowed_hosts = vec!["127.0.0.1".to_owned()];
    // A receiver that answers 500 every time: fast failures, no timeout to wait out.
    let (port, _hits) = receiver("500 Internal Server Error").await;

    let db = h.tdb.db();
    let id = darkhold_db::marketplace::create_webhook(
        db,
        "owner-failing",
        &format!("http://127.0.0.1:{port}/hook"),
        "whsec_failing",
        &["*".to_string()],
    )
    .await
    .expect("create webhook");
    test_support::exec(
        db,
        "UPDATE webhook_endpoints SET rate_per_minute = 100000 WHERE id = ?",
        "UPDATE webhook_endpoints SET rate_per_minute = 100000 WHERE id = $1",
        &id,
    )
    .await;

    let state = AppState::new(h.config.clone(), db.clone());
    let threshold = darkhold_domain::webhook::DEFAULT_FAILURE_THRESHOLD;
    for n in 0..threshold {
        darkhold_app::webhook_delivery::deliver_notification(&state, &event(&format!("evt-{n}")))
            .await
            .expect("deliver");
    }

    let subs = darkhold_db::webhooks::subscriptions(db)
        .await
        .expect("subscriptions");
    let row = subs.iter().find(|r| r["id"] == json!(id)).expect("row");
    assert_eq!(
        row["active"],
        json!(false),
        "a permanently failing receiver must be switched off by the sender itself: {row}"
    );
    assert!(
        row["disabled_reason"]
            .as_str()
            .unwrap_or_default()
            .contains("consecutive"),
        "{row}"
    );
}
