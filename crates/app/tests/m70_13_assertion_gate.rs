//! §20.9.1's eligibility setting is enforced, not merely configured (M70-13).
//!
//! `record_assertion` and `get_assertion` had no callers and `Rules::assertion_required` had none either — a tested rule with
//! no reader. The consequence was not cosmetic: an instance configured `original` (ADR 0017's default) would price and sell
//! any work at all, and `any-with-assertion` promised a rights assertion that nothing ever collected or checked.
//!
//! Three doors now exist and one rule decides all of them:
//!
//! * `POST /works/{id}/assertion` — the author records what they are claiming. Only the work's owner, because a rights claim
//!   by somebody else is a statement about the author rather than an assertion by them.
//! * `GET /works/{id}/assertion` — public, with `asserted: false` rather than a 404 when there is none: "nobody asserted" is
//!   the answer to a question about a work that exists.
//! * The gates in `set_pricing` and `purchase`, which ask the same helper — so what counts as an assertion cannot drift
//!   between the price and the sale.
//!
//! The two modes differ in *which* assertion suffices, and that is the assertion this file spends most of its length on: on an
//! `original` instance a `rights-held` claim is not enough, and a test that only checked "an assertion exists" would pass a
//! fanwork onto the shelf §20.9.1 exists to keep it off.

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

fn config_with(eligibility: &str) -> Config {
    let mut config = Config::development_defaults();
    config.monetization.eligibility = eligibility.to_owned();
    // This test exercises one account through three eligibility settings in a single run — three publications, four
    // assertions and three pricing attempts — and the development write quota is sized for a person, not for a fixture. The
    // limits themselves are proved by their own suites; raising them here keeps this one about §20.9.1.
    config.rate_limits.write.burst = config.rate_limits.write.burst.saturating_mul(10);
    config.rate_limits.write.per_minute = config.rate_limits.write.per_minute.saturating_mul(10);
    config.rate_limits.default.burst = config.rate_limits.default.burst.saturating_mul(10);
    config.rate_limits.default.per_minute =
        config.rate_limits.default.per_minute.saturating_mul(10);
    config
}

/// A published work, owned by `author`.
async fn published_work(author: &mut TestClient, title: &str) -> String {
    let (status, body) = author
        .post("/api/v1/works", json!({ "title": title }))
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
                    { "type": "paragraph", "content": [{ "type": "text", "text": "Words that can be sold." }] },
                ]},
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = author
        .post(
            &format!("/api/v1/works/{work}/publish"),
            json!({ "expected_version": version, "idempotency_key": format!("as-{work}") }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    work
}

async fn price(client: &mut TestClient, work: &str) -> (StatusCode, Value) {
    client
        .post(
            &format!("/api/v1/works/{work}/pricing"),
            json!({ "model": "purchase", "price_minor": 400, "currency": "EUR", "public_at_offset": Value::Null }),
        )
        .await
}

#[tokio::test]
async fn an_original_instance_will_not_price_a_fanwork_assertion() {
    let dir = scratch_dir("assertion-gate");
    let tdb = TestDb::connect_with_dir("m7013-assertion-gate", &dir).await;
    let db: Database = tdb.db().clone();

    // Default: ADR 0017's `original`.
    let config = config_with("original");
    let mut author = TestClient::new(build(&config, &db));
    register(&mut author, "as-author@example.test", "asauthor").await;
    sign_in_as(&mut author, &tdb, "as-author@example.test", "asauthor").await;
    let mut stranger = TestClient::new(build(&config, &db));
    register(&mut stranger, "as-stranger@example.test", "asstranger").await;
    sign_in_as(
        &mut stranger,
        &tdb,
        "as-stranger@example.test",
        "asstranger",
    )
    .await;

    let work = published_work(&mut author, "A Work To Price").await;

    // 1. A work nobody has asserted anything about cannot be priced, and the refusal names the field a client must set.
    let (status, body) = price(&mut author, &work).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["error"]["field_errors"]["assertion"].is_string(),
        true,
        "{body}"
    );
    assert!(
        body["error"]["field_errors"]["assertion"]
            .as_str()
            .unwrap_or_default()
            .contains("original"),
        "the refusal says which claim this instance wants: {body}"
    );

    // 2. Nobody has asserted, and the public read says so rather than 404-ing.
    let (status, body) = stranger
        .get(format!("/api/v1/works/{work}/assertion"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["asserted"], json!(false), "{body}");
    assert_eq!(body["kind"], Value::Null, "{body}");

    // 3. Somebody else's claim about this work is not an assertion: it is a statement about the author.
    let (status, body) = stranger
        .post(
            &format!("/api/v1/works/{work}/assertion"),
            json!({ "kind": "original" }),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // 4. A kind that is not in the vocabulary is refused by field.
    let (status, body) = author
        .post(
            &format!("/api/v1/works/{work}/assertion"),
            json!({ "kind": "i-promise" }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["error"]["field_errors"]["kind"].is_string(),
        true,
        "{body}"
    );

    // 5. **The mode distinction.** A `rights-held` assertion is stored, readable, and still not enough on an `original`
    //    instance — which is the whole of what §20.9.1's stricter mode is for.
    let (status, body) = author
        .post(
            &format!("/api/v1/works/{work}/assertion"),
            json!({ "kind": "rights-held" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["kind"], json!("rights-held"), "{body}");
    assert!(
        body["policy_version"]
            .as_str()
            .unwrap_or_default()
            .contains("monetization"),
        "the server owns the policy version it stored: {body}"
    );
    let (status, body) = price(&mut author, &work).await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a fanwork assertion does not satisfy an original instance: {body}"
    );

    // 6. The `original` claim satisfies it, and the public read shows the newest claim.
    let (status, body) = author
        .post(
            &format!("/api/v1/works/{work}/assertion"),
            json!({ "kind": "original" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = stranger
        .get(format!("/api/v1/works/{work}/assertion"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["asserted"], json!(true), "{body}");
    let (status, body) = price(&mut author, &work).await;
    assert!(status.is_success(), "the work can now be priced: {body}");

    // 7. And it can be bought — the sale asks the same question the price did.
    let mut buyer = TestClient::new(build(&config, &db));
    register(&mut buyer, "as-buyer@example.test", "asbuyer").await;
    sign_in_as(&mut buyer, &tdb, "as-buyer@example.test", "asbuyer").await;
    let (status, body) = buyer
        .post(&format!("/api/v1/works/{work}/purchase"), json!({}))
        .await;
    assert!(status.is_success(), "{body}");

    // 8. Widening the setting to `any-with-assertion` accepts the `rights-held` claim that the strict mode refused.
    let wide = config_with("any-with-assertion");
    let mut wide_author = TestClient::new(build(&wide, &db));
    sign_in_as(&mut wide_author, &tdb, "as-author@example.test", "asauthor").await;
    let fanwork = published_work(&mut wide_author, "A Fanwork To Price").await;
    let (status, body) = wide_author
        .post(
            &format!("/api/v1/works/{fanwork}/assertion"),
            json!({ "kind": "rights-held" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = price(&mut wide_author, &fanwork).await;
    assert!(
        status.is_success(),
        "rights-held satisfies any-with-assertion: {body}"
    );

    // 9. **A price set under a looser setting does not inherit permission from it.** The fanwork was priced while the
    //    instance accepted `rights-held`; a sale asked under `original` must refuse, or the stricter setting would only
    //    apply to works priced after it was switched on.
    let mut strict_buyer = TestClient::new(build(&config, &db));
    register(&mut strict_buyer, "as-strict@example.test", "asstrict").await;
    sign_in_as(
        &mut strict_buyer,
        &tdb,
        "as-strict@example.test",
        "asstrict",
    )
    .await;
    let (status, body) = strict_buyer
        .post(&format!("/api/v1/works/{fanwork}/purchase"), json!({}))
        .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "the sale asks the same question the price was asked: {body}"
    );
    assert_eq!(
        body["error"]["field_errors"]["work_id"].is_string(),
        true,
        "{body}"
    );

    // 10. `disabled` sells nothing, and says so on the field that selects what is sold — credit tips are still allowed and
    //    are not this door's question.
    let off = config_with("disabled");
    let mut off_author = TestClient::new(build(&off, &db));
    sign_in_as(&mut off_author, &tdb, "as-author@example.test", "asauthor").await;
    let unsold = published_work(&mut off_author, "A Work Not For Sale").await;
    let (status, body) = off_author
        .post(
            &format!("/api/v1/works/{unsold}/assertion"),
            json!({ "kind": "original" }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "an assertion is still recordable: {body}"
    );
    let (status, body) = price(&mut off_author, &unsold).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["error"]["field_errors"]["model"].is_string(),
        true,
        "{body}"
    );
}
