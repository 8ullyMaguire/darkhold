//! The device doors (M70-13): `user_devices` had three store functions and no surface.
//!
//! A device is how a reader receives an export or a push. The three facts worth asserting are the ones a leak or a
//! takeover would violate: the push subscription is never echoed back, one account cannot see or delete another's
//! device, and re-registering the same client-chosen id updates rather than duplicating.

use axum::http::StatusCode;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::Database;
use test_support::{id, register, scratch_dir, TestClient, TestDb};

fn build(config: &Config, db: &Database) -> axum::Router {
    server::build_router(AppState::new(config.clone(), db.clone()))
}

#[tokio::test]
async fn a_device_is_the_readers_own_and_its_push_subscription_never_comes_back() {
    let dir = scratch_dir("devices");
    let tdb = TestDb::connect_with_dir("m7013-devices", &dir).await;
    let db: Database = tdb.db().clone();

    let mut config = Config::development_defaults();
    config.storage.root = dir.clone();

    let mut reader = TestClient::new(build(&config, &db));
    register(&mut reader, "devices-reader@example.test", "devicesreader").await;

    // 1. No devices yet.
    let (status, body) = reader.get("/api/v1/devices").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["devices"].as_array().map(Vec::len), Some(0), "{body}");

    // 2. Register one, with a subscription the size of a real one. The id is a UUID because the column is `UUID` on
    //    PostgreSQL — a client-chosen id that decodes on SQLite and fails on the other engine is not an id.
    let device = id("device-one");

    // 3. A non-UUID id is refused by name rather than 500ing on the engine that types the column.
    let (status, body) = reader
        .post(
            "/api/v1/devices",
            serde_json::json!({"id": "device-one", "label": "Phone"}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["error"]["field_errors"]["id"],
        serde_json::json!("the string crypto.randomUUID() returns"),
        "{body}"
    );
    let subscription = serde_json::json!({
        "endpoint": "https://push.example.test/abc",
        "keys": {"p256dh": "BOr...", "auth": "secret"},
    });
    let (status, body) = reader
        .post(
            "/api/v1/devices",
            serde_json::json!({"id": device, "label": "Phone", "push_subscription": subscription}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["id"], serde_json::json!(device), "{body}");
    assert_eq!(body["has_push_subscription"], serde_json::json!(true), "{body}");

    // 3. The subscription is a delivery credential, so the list reports its presence and not its value.
    let (status, body) = reader.get("/api/v1/devices").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let devices = body["devices"].as_array().expect("array");
    assert_eq!(devices.len(), 1, "{body}");
    assert!(
        !body.to_string().contains("push.example.test"),
        "the endpoint must not be echoed: {body}"
    );
    assert!(
        !body.to_string().contains("p256dh"),
        "the keys must not be echoed: {body}"
    );

    // 4. Re-registering the same id updates the row rather than adding a second device.
    let (status, _) = reader
        .post(
            "/api/v1/devices",
            serde_json::json!({"id": device, "label": "Phone (new)"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (_, body) = reader.get("/api/v1/devices").await;
    let devices = body["devices"].as_array().expect("array");
    assert_eq!(devices.len(), 1, "an update is not a second device: {body}");
    assert_eq!(devices[0]["label"], serde_json::json!("Phone (new)"), "{body}");

    // 5. Another account cannot see it, cannot delete it, and cannot learn it exists.
    let mut other = TestClient::new(build(&config, &db));
    register(&mut other, "devices-other@example.test", "devicesother").await;
    let (_, body) = other.get("/api/v1/devices").await;
    assert_eq!(body["devices"].as_array().map(Vec::len), Some(0), "{body}");
    let (status, body) = other.request("DELETE", &format!("/api/v1/devices/{device}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (_, body) = reader.get("/api/v1/devices").await;
    assert_eq!(
        body["devices"].as_array().map(Vec::len),
        Some(1),
        "the device must survive somebody else's delete: {body}"
    );

    // 6. The owner deletes it, and it is gone.
    let (status, body) = reader.request("DELETE", &format!("/api/v1/devices/{device}"), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, body) = reader.get("/api/v1/devices").await;
    assert_eq!(body["devices"].as_array().map(Vec::len), Some(0), "{body}");

    // 7. A subscription that is not an object is refused by name.
    let (status, body) = reader
        .post(
            "/api/v1/devices",
            serde_json::json!({"id": id("device-two"), "push_subscription": "not-an-object"}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["error"]["field_errors"]["push_subscription"],
        serde_json::json!("the object the browser's PushManager returned"),
        "{body}"
    );
}
