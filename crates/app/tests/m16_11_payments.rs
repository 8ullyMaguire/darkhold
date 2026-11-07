//! M16-11 — the paid marketplace end to end.
//!
//! The arithmetic has its own unit tests in `darkhold_domain::payments`. This file proves the sequence the money actually takes: a purchase is
//! **pending** and grants nothing, capture grants the package and posts the seller's share, a full refund takes both back, a partial refund
//! takes neither, and a retried purchase does not charge twice.
//!
//! A note on why the capture step is tested rather than assumed: there is no payment provider, so a host that granted access at purchase time
//! would be giving away paid packages in exchange for a form post. The test that proves it is the one that asserts `granted: false` and no
//! grant row *before* capture.

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
        "darkhold-m1611-{tag}-{}-{:?}",
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

    /// Insert a published package owned by `seller`.
    async fn publish(&self, id: &str, version: &str, seller: &str) -> String {
        let document = json!({
            "id": id,
            "version": 1,
            "category": "integration",
            "entrypoint": "main.wasm",
            "required_host_api_version": 1,
            "permissions": ["work.read"],
            "resource_limits": { "memory_mib": 32, "fuel": 1_000_000, "host_calls_per_run": 64 },
            "supported_surfaces": ["library"],
            "license": "MIT",
            "network_allowlist": [],
            "pricing": "paid",
        })
        .to_string();
        sqlx::query(
            "INSERT INTO extension_manifests (id, version, document, submitted_by, state, created_at)
             VALUES (?, ?, ?, ?, 'approved', '2026-11-01T00:00:00Z')",
        )
        .bind(id)
        .bind(version)
        .bind(&document)
        .bind(seller)
        .execute(self.tdb.db().sqlite_pool().expect("sqlite"))
        .await
        .expect("insert manifest");
        document
    }

    async fn seller_earnings(&self, account: &str) -> i64 {
        let row: (i64,) = sqlx::query_as(
            "SELECT COALESCE(SUM(amount_minor), 0) FROM author_earnings_ledger WHERE author_account_id = ?",
        )
        .bind(account)
        .fetch_one(self.tdb.db().sqlite_pool().expect("sqlite"))
        .await
        .expect("earnings sum");
        row.0
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

    fn capture_cookies(&mut self, response: &axum::response::Response) {
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
        self.capture_cookies(&response);
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
                    "email": format!("m16-11-{handle}@example.test"),
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
        me["account"]["id"]
            .as_str()
            .or_else(|| me["id"].as_str())
            .expect("account id")
            .to_owned()
    }
}

fn message(body: &Value) -> String {
    body["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

/// A client whose session carries `role:operator`, which is what `require_operator` reads.
async fn operator(h: &Harness, handle: &str) -> Client {
    let mut client = h.client();
    let account = client.register(handle).await;
    sqlx::query("UPDATE sessions SET roles = '[\"role:operator\"]' WHERE account_id = ?")
        .bind(&account)
        .execute(h.tdb.db().sqlite_pool().expect("sqlite"))
        .await
        .expect("grant operator role");
    client
}

#[tokio::test]
async fn a_purchase_grants_nothing_until_the_money_exists() {
    let h = Harness::new("capture").await;
    let mut seller = h.client();
    let seller_account = seller.register("seller").await;
    h.publish("shelf-sorter", "1.0.0", &seller_account).await;

    let (status, body) = seller
        .send(
            "PUT",
            "/api/v1/operator/extensions/shelf-sorter/price",
            Some(json!({ "price_minor": 1_000, "currency": "EUR" })),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a seller may price their own package: {body}"
    );

    // Tax profile first, so the purchase can say what part of the price is tax.
    let mut op = operator(&h, "tax-op").await;
    let (status, body) = op
        .send(
            "PUT",
            "/api/v1/operator/tax-profile",
            Some(json!({
                "legal_name": "Archive Ltd",
                "country": "de",
                "tax_id": "DE123456789",
                "rate_bp": 1_900,
                "invoice_prefix": "DH"
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["country"], "DE",
        "the code is stored upper-case: {body}"
    );

    let mut buyer = h.client();
    let buyer_account = buyer.register("buyer").await;
    let (status, body) = buyer
        .send(
            "POST",
            "/api/v1/extensions/shelf-sorter/purchase",
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["purchase_state"], "pending", "{body}");
    assert_eq!(body["payment"]["amount_minor"], 1_000, "{body}");
    assert_eq!(
        body["tax_minor"], 160,
        "19% inclusive of 1000 is 160: {body}"
    );
    assert_eq!(body["net_minor"], 840, "{body}");
    assert_eq!(
        body["processor"]["captured"], false,
        "a manual processor never claims to have taken money: {body}"
    );
    let payment_id = body["payment"]["id"]
        .as_str()
        .expect("payment id")
        .to_owned();

    // Nothing was granted, and nothing was earned.
    assert!(
        !darkhold_db::marketplace::holds_grant(
            h.tdb.db(),
            &buyer_account,
            "shelf-sorter",
            "work.read"
        )
        .await
        .expect("read"),
        "a pending payment must not grant the package"
    );
    assert_eq!(h.seller_earnings(&seller_account).await, 0);

    let (status, body) = op
        .send(
            "POST",
            &format!("/api/v1/operator/payments/{payment_id}/capture"),
            Some(json!({ "reference": "bank-transfer-4471" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["granted"], true, "{body}");
    assert_eq!(body["reference"], "bank-transfer-4471", "{body}");
    assert_eq!(
        body["split"]["developer_minor"], 714,
        "85% of the 840 net is 714: {body}"
    );
    assert_eq!(body["split"]["host_minor"], 126, "15% of 840: {body}");

    assert!(
        darkhold_db::marketplace::holds_grant(
            h.tdb.db(),
            &buyer_account,
            "shelf-sorter",
            "work.read"
        )
        .await
        .expect("read"),
        "capture is the moment the package becomes the buyer's"
    );
    assert_eq!(h.seller_earnings(&seller_account).await, 714);

    // Capturing twice is refused rather than double-charging.
    let (status, body) = op
        .send(
            "POST",
            &format!("/api/v1/operator/payments/{payment_id}/capture"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(message(&body).contains("captured"), "{}", message(&body));
    assert_eq!(h.seller_earnings(&seller_account).await, 714);
}

#[tokio::test]
async fn a_retried_purchase_returns_the_same_payment() {
    let h = Harness::new("retry").await;
    let mut seller = h.client();
    let seller_account = seller.register("retry-seller").await;
    h.publish("shelf-sorter", "1.0.0", &seller_account).await;
    seller
        .send(
            "PUT",
            "/api/v1/operator/extensions/shelf-sorter/price",
            Some(json!({ "price_minor": 500, "currency": "EUR" })),
        )
        .await;

    let mut buyer = h.client();
    buyer.register("retry-buyer").await;
    let (_, first) = buyer
        .send(
            "POST",
            "/api/v1/extensions/shelf-sorter/purchase",
            Some(json!({ "idempotency_key": "click-1" })),
        )
        .await;
    let (status, second) = buyer
        .send(
            "POST",
            "/api/v1/extensions/shelf-sorter/purchase",
            Some(json!({ "idempotency_key": "click-1" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(
        first["payment"]["id"], second["payment"]["id"],
        "a retry is the same payment, not a second charge: {first} {second}"
    );

    let (status, body) = buyer.send("GET", "/api/v1/me/payments", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["payments"].as_array().map(Vec::len), Some(1), "{body}");
}

#[tokio::test]
async fn a_full_refund_takes_the_package_and_the_revenue_back() {
    let h = Harness::new("refund-full").await;
    let mut seller = h.client();
    let seller_account = seller.register("refund-seller").await;
    h.publish("shelf-sorter", "1.0.0", &seller_account).await;
    seller
        .send(
            "PUT",
            "/api/v1/operator/extensions/shelf-sorter/price",
            Some(json!({ "price_minor": 1_000, "currency": "EUR" })),
        )
        .await;

    let mut buyer = h.client();
    let buyer_account = buyer.register("refund-buyer").await;
    let (_, body) = buyer
        .send(
            "POST",
            "/api/v1/extensions/shelf-sorter/purchase",
            Some(json!({})),
        )
        .await;
    let payment_id = body["payment"]["id"]
        .as_str()
        .expect("payment id")
        .to_owned();

    let mut op = operator(&h, "refund-op").await;
    op.send(
        "POST",
        &format!("/api/v1/operator/payments/{payment_id}/capture"),
        Some(json!({})),
    )
    .await;
    assert_eq!(
        h.seller_earnings(&seller_account).await,
        850,
        "85% of 1000 with no tax"
    );

    // A reason is required: a refund nobody can explain is the ledger entry that starts an argument.
    let (status, body) = op
        .send(
            "POST",
            &format!("/api/v1/operator/payments/{payment_id}/refund"),
            Some(json!({ "reason": "   " })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    let (status, body) = op
        .send(
            "POST",
            &format!("/api/v1/operator/payments/{payment_id}/refund"),
            Some(json!({ "reason": "the package stopped working" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["full"], true, "{body}");
    assert_eq!(body["access_revoked"], true, "{body}");
    assert_eq!(body["refunded_total_minor"], 1_000, "{body}");

    assert!(
        !darkhold_db::marketplace::holds_grant(
            h.tdb.db(),
            &buyer_account,
            "shelf-sorter",
            "work.read"
        )
        .await
        .expect("read"),
        "a full refund takes the package back"
    );
    assert_eq!(
        h.seller_earnings(&seller_account).await,
        0,
        "and reverses the seller's share, because the sale did not happen"
    );
}

#[tokio::test]
async fn a_partial_refund_keeps_access_and_cannot_exceed_the_charge() {
    let h = Harness::new("refund-partial").await;
    let mut seller = h.client();
    let seller_account = seller.register("partial-seller").await;
    h.publish("shelf-sorter", "1.0.0", &seller_account).await;
    seller
        .send(
            "PUT",
            "/api/v1/operator/extensions/shelf-sorter/price",
            Some(json!({ "price_minor": 1_000, "currency": "EUR" })),
        )
        .await;

    let mut buyer = h.client();
    let buyer_account = buyer.register("partial-buyer").await;
    let (_, body) = buyer
        .send(
            "POST",
            "/api/v1/extensions/shelf-sorter/purchase",
            Some(json!({})),
        )
        .await;
    let payment_id = body["payment"]["id"]
        .as_str()
        .expect("payment id")
        .to_owned();
    let mut op = operator(&h, "partial-op").await;
    op.send(
        "POST",
        &format!("/api/v1/operator/payments/{payment_id}/capture"),
        Some(json!({})),
    )
    .await;

    let (status, body) = op
        .send(
            "POST",
            &format!("/api/v1/operator/payments/{payment_id}/refund"),
            Some(json!({ "amount_minor": 100, "reason": "goodwill" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["full"], false, "{body}");
    assert_eq!(body["access_revoked"], false, "{body}");
    assert!(
        darkhold_db::marketplace::holds_grant(
            h.tdb.db(),
            &buyer_account,
            "shelf-sorter",
            "work.read"
        )
        .await
        .expect("read"),
        "a goodwill refund is not a return: the reader keeps what they bought"
    );

    // More than remains is refused with both numbers, not clamped.
    let (status, body) = op
        .send(
            "POST",
            &format!("/api/v1/operator/payments/{payment_id}/refund"),
            Some(json!({ "amount_minor": 2_000, "reason": "too much" })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        message(&body).contains("900"),
        "the refusal names what is left: {}",
        message(&body)
    );

    // The rest completes it, and completing it is what revokes.
    let (status, body) = op
        .send(
            "POST",
            &format!("/api/v1/operator/payments/{payment_id}/refund"),
            Some(json!({ "amount_minor": 900, "reason": "returned the rest" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["full"], true, "{body}");
    assert!(!darkhold_db::marketplace::holds_grant(
        h.tdb.db(),
        &buyer_account,
        "shelf-sorter",
        "work.read"
    )
    .await
    .expect("read"));
}

#[tokio::test]
async fn a_package_cannot_be_priced_against_its_author_and_an_author_cannot_buy_it() {
    let h = Harness::new("self-dealing").await;
    let mut seller = h.client();
    let seller_account = seller.register("own-seller").await;
    h.publish("shelf-sorter", "1.0.0", &seller_account).await;
    seller
        .send(
            "PUT",
            "/api/v1/operator/extensions/shelf-sorter/price",
            Some(json!({ "price_minor": 1_000, "currency": "EUR" })),
        )
        .await;

    // A stranger cannot price someone else's package.
    let mut stranger = h.client();
    stranger.register("stranger").await;
    let (status, body) = stranger
        .send(
            "PUT",
            "/api/v1/operator/extensions/shelf-sorter/price",
            Some(json!({ "price_minor": 1, "currency": "EUR" })),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    // And the author cannot buy their own package.
    let (status, body) = seller
        .send(
            "POST",
            "/api/v1/extensions/shelf-sorter/purchase",
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        message(&body).contains("cannot buy it"),
        "{}",
        message(&body)
    );

    // An unpriced package is not for sale at all.
    h.publish("unpriced", "1.0.0", &seller_account).await;
    let (status, body) = stranger
        .send(
            "POST",
            "/api/v1/extensions/unpriced/purchase",
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        message(&body).contains("not for sale"),
        "{}",
        message(&body)
    );
}
