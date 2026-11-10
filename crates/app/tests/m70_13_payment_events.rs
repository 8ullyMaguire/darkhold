//! The fee ledger records money that moved, once, and the instance publishes what it cost (M70-13).
//!
//! `record_payment_event` had no caller, and the table it writes is the one §20.9.5 makes a disclosure obligation: *every*
//! monetary transaction records the processor's fee, and the range actually paid over ninety days is published on
//! `/api/v1/meta` (§20.10.7) so the solidarity arithmetic is transparent about what processing costs. With no caller the
//! table was empty forever and the published range could only have been invented.
//!
//! Three things had to be true before the write could honestly be made:
//!
//! * **The path is a money path.** A purchase, a money tip and a payout move money; a credit tip moves credits between buckets
//!   and records nothing here, because a credit is not a payment and a fee booked against one would be a fee nobody paid.
//! * **The write is repeatable.** `payment_events` had no idempotency key — migration 0200 adds `event_key` — so a retried tip
//!   (whose earnings row is idempotent) no longer becomes a second event that inflates the published fee range.
//! * **Zero is stated, not implied.** There is no processor adapter in this binary, so every recorded fee is zero. `wired:
//!   false` on the same document is what keeps `0` from being read as "processing is free": it says "nothing has been charged
//!   by a processor, and none is wired to charge one".

use axum::http::StatusCode;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::Database;
use serde_json::{json, Value};
use test_support::{register, scratch_dir, sign_in_as, TestClient, TestDb};

fn build(config: &Config, db: &Database) -> axum::Router {
    server::build_router(AppState::new(config.clone(), db.clone()))
}

async fn event_count(db: &Database, key: &str) -> i64 {
    match db.backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query_scalar("SELECT COUNT(*) FROM payment_events WHERE event_key = ?")
                .bind(key)
                .fetch_one(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("count")
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query_scalar("SELECT COUNT(*) FROM payment_events WHERE event_key = $1")
                .bind(key)
                .fetch_one(db.postgres_pool().expect("postgres"))
                .await
                .expect("count")
        }
    }
}

/// A published work with a price on it — the shortest road to a purchase.
async fn a_work_for_sale(author: &mut TestClient, price: i64) -> String {
    let (status, body) = author
        .post("/api/v1/works", json!({ "title": "For Sale" }))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let work = body["id"].as_str().expect("work").to_owned();
    let version = body["version"].as_i64().expect("version");

    let (status, body) = author
        .post(
            &format!("/api/v1/works/{work}/chapters"),
            json!({ "title": "Chapter One" }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let unit = body["id"].as_str().expect("unit").to_owned();
    let unit_version = body["version"].as_i64().expect("unit version");
    let (status, body) = author
        .request(
            "PATCH",
            &format!("/api/v1/chapters/{unit}"),
            Some(json!({
                "expected_version": unit_version,
                "document": { "type": "doc", "content": [
                    { "type": "paragraph", "content": [{ "type": "text", "text": "A chapter worth buying." }] },
                ]},
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = author
        .post(
            &format!("/api/v1/works/{work}/publish"),
            json!({ "expected_version": version, "idempotency_key": format!("pe-{work}") }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // §20.9.1: this instance's default mode is `original`, so the author must have asserted originality before the work may
    // carry a price. Pricing without it is refused by field, which is the gate's own test's subject.
    let (status, body) = author
        .post(
            &format!("/api/v1/works/{work}/assertion"),
            json!({ "kind": "original" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "assertion: {body}");

    let (status, body) = author
        .post(
            &format!("/api/v1/works/{work}/pricing"),
            json!({ "model": "purchase", "price_minor": price, "currency": "EUR", "public_at_offset": Value::Null }),
        )
        .await;
    assert!(status.is_success(), "pricing: {body}");
    work
}

async fn payments(anonymous: &mut TestClient) -> Value {
    let (status, meta) = anonymous.get("/api/v1/meta").await;
    assert_eq!(status, StatusCode::OK, "{meta}");
    meta["payments"].clone()
}

#[tokio::test]
async fn a_fee_range_is_published_only_when_a_payment_was_actually_recorded() {
    let dir = scratch_dir("payment-events");
    let tdb = TestDb::connect_with_dir("m7013-payment-events", &dir).await;
    let db: Database = tdb.db().clone();
    let config = Config::development_defaults();

    let mut anonymous = TestClient::new(build(&config, &db));

    // 1. Nothing has been paid: the range is null, not zero. "No processor has charged us" is not "processing is free".
    let before = payments(&mut anonymous).await;
    assert_eq!(before["window_days"], json!(90), "{before}");
    assert_eq!(before["processor_fee_min_minor"], Value::Null, "{before}");
    assert_eq!(before["processor_fee_max_minor"], Value::Null, "{before}");
    assert_eq!(before["processor_wired"], json!(false), "{before}");

    // 2. A purchase records one payment event, and the published range stops being null.
    let mut author = TestClient::new(build(&config, &db));
    register(&mut author, "pe-author@example.test", "peauthor").await;
    sign_in_as(&mut author, &tdb, "pe-author@example.test", "peauthor").await;
    let work = a_work_for_sale(&mut author, 500).await;

    let mut buyer = TestClient::new(build(&config, &db));
    register(&mut buyer, "pe-buyer@example.test", "pebuyer").await;
    sign_in_as(&mut buyer, &tdb, "pe-buyer@example.test", "pebuyer").await;
    let (status, body) = buyer
        .post(&format!("/api/v1/works/{work}/purchase"), json!({}))
        .await;
    assert!(status.is_success(), "{body}");

    let after = payments(&mut anonymous).await;
    assert_eq!(after["processor_fee_min_minor"], json!(0), "{after}");
    assert_eq!(after["processor_fee_max_minor"], json!(0), "{after}");
    assert_eq!(after["processor_wired"], json!(false), "{after}");

    // 3. The purchase is idempotent *and* so is its payment event: buying again (already entitled) records nothing new.
    let (status, _) = buyer
        .post(&format!("/api/v1/works/{work}/purchase"), json!({}))
        .await;
    assert!(status.is_success());
    let purchase_key_prefix = "purchase:";
    let key = match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar::<_, String>(
            "SELECT event_key FROM payment_events WHERE event_key LIKE ? ORDER BY created_at DESC",
        )
        .bind(format!("{purchase_key_prefix}%"))
        .fetch_one(db.sqlite_pool().expect("sqlite"))
        .await
        .expect("key"),
        darkhold_db::Backend::Postgres => sqlx::query_scalar::<_, String>(
            "SELECT event_key FROM payment_events WHERE event_key LIKE $1 ORDER BY created_at DESC",
        )
        .bind(format!("{purchase_key_prefix}%"))
        .fetch_one(db.postgres_pool().expect("postgres"))
        .await
        .expect("key"),
    };
    assert_eq!(
        event_count(&db, &key).await,
        1,
        "a second purchase of an owned work is not a second payment"
    );
    // The event records the money that moved, not just that something moved: gross is the price, the fee is zero because no
    // processor took one, and net is what the author's split is computed on.
    let (kind, gross, net): (String, i64, i64) = match db.backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query_as("SELECT kind, amount_minor, net_minor FROM payment_events WHERE event_key = ?")
                .bind(&key)
                .fetch_one(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("event")
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query_as("SELECT kind, amount_minor::bigint, net_minor::bigint FROM payment_events WHERE event_key = $1")
                .bind(&key)
                .fetch_one(db.postgres_pool().expect("postgres"))
                .await
                .expect("event")
        }
    };
    assert_eq!(kind, "purchase");
    assert_eq!(
        gross, 500,
        "the event's gross is the price that was charged"
    );
    assert_eq!(net, 500, "with no processor fee, net is gross");

    // 4. A money tip records its own event, keyed by the tip's idempotency key — and a retried tip does not double-record,
    //    even though the tip path itself is idempotent only in the earnings ledger.
    let tip_key = format!(
        "tip:{}-{}:EUR:250",
        buyer_account(&db, "pe-buyer@example.test").await,
        work
    );
    let tip = json!({ "amount_minor": 250, "currency": "EUR", "channel": "money" });
    let (status, body) = buyer
        .post(&format!("/api/v1/works/{work}/tips"), tip.clone())
        .await;
    assert!(status.is_success(), "{body}");
    let (status, body) = buyer
        .post(&format!("/api/v1/works/{work}/tips"), tip.clone())
        .await;
    assert!(status.is_success(), "{body}");
    assert_eq!(
        event_count(&db, &tip_key).await,
        1,
        "the retried tip is one payment"
    );

    // 5. A payout is a payment too, keyed by the payout's id.
    let (status, body) = author
        .post(
            "/api/v1/me/payouts",
            json!({ "amount_minor": 100, "currency": "EUR", "processor_reference": "proc-1" }),
        )
        .await;
    assert!(status.is_success(), "{body}");
    let payout_id = body["id"].as_str().expect("payout id").to_owned();
    assert_eq!(
        event_count(&db, &format!("payout:{payout_id}")).await,
        1,
        "the payout is recorded"
    );

    // 6. Credit tips are not payments: they move credits between buckets, and no fee is booked against them.
    let credits_before: i64 = match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar("SELECT COUNT(*) FROM payment_events")
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("count"),
        darkhold_db::Backend::Postgres => sqlx::query_scalar("SELECT COUNT(*) FROM payment_events")
            .fetch_one(db.postgres_pool().expect("postgres"))
            .await
            .expect("count"),
    };
    let (status, body) = buyer
        .post(
            &format!("/api/v1/works/{work}/tips"),
            json!({ "amount_minor": 5, "currency": "EUR", "channel": "credit" }),
        )
        .await;
    assert!(status.is_success(), "{body}");
    let credits_after: i64 = match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar("SELECT COUNT(*) FROM payment_events")
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("count"),
        darkhold_db::Backend::Postgres => sqlx::query_scalar("SELECT COUNT(*) FROM payment_events")
            .fetch_one(db.postgres_pool().expect("postgres"))
            .await
            .expect("count"),
    };
    assert_eq!(
        credits_after, credits_before,
        "a credit tip is not a monetary transaction and records no processor fee"
    );
}

/// The account id behind a handle, for building the tip idempotency key the route builds.
async fn buyer_account(db: &Database, email: &str) -> String {
    match db.backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query_scalar::<_, String>("SELECT id FROM accounts WHERE email = ?")
                .bind(email)
                .fetch_one(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("account")
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query_scalar::<_, String>("SELECT id::text FROM accounts WHERE email = $1")
                .bind(email)
                .fetch_one(db.postgres_pool().expect("postgres"))
                .await
                .expect("account")
        }
    }
}
