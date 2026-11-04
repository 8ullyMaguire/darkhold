//! M45-50 — the store half of a scheduled taste vector.
//!
//! What is pinned here is the part an operator would get wrong by hand: the window is half-open, an
//! overlapping window is resolved by the one that started last, a revoked window stops applying
//! without disappearing, and another instance's windows are invisible.

use darkhold_db::theme_schedule;
use serde_json::json;
use test_support::TestDb;

/// A migrated database on SQLite, or on PostgreSQL when `DARKHOLD_TEST_PG_URL` is set.
async fn db(tag: &str) -> TestDb {
    let dir = test_support::scratch_dir(tag);
    TestDb::connect_with_dir(tag, &dir).await
}

const NOW: &str = "2026-11-04T12:00:00Z";

#[tokio::test]
async fn a_window_applies_from_its_start_and_stops_at_its_end() {
    let tdb = db("boundaries").await;
    let id = theme_schedule::create(
        tdb.db(),
        "example.test",
        "Christmas",
        &json!({ "christmas": 5.0 }),
        "2026-12-24T00:00:00Z",
        "2026-12-26T00:00:00Z",
        NOW,
    )
    .await
    .expect("create");

    let active = theme_schedule::active_at(tdb.db(), "example.test", "2026-12-25T00:00:00Z")
        .await
        .expect("query")
        .expect("the window covers Christmas day");
    assert_eq!(active.id, id);
    assert_eq!(active.label, "Christmas");
    assert_eq!(active.theme_vector, json!({ "christmas": 5.0 }));

    assert!(
        theme_schedule::active_at(tdb.db(), "example.test", "2026-12-23T23:59:59Z")
            .await
            .expect("query")
            .is_none(),
        "before the window"
    );
    assert!(
        theme_schedule::active_at(tdb.db(), "example.test", "2026-12-26T00:00:00Z")
            .await
            .expect("query")
            .is_none(),
        "the end instant is not inside the window"
    );
}

#[tokio::test]
async fn when_two_windows_overlap_the_one_that_started_last_wins() {
    let tdb = db("overlap").await;
    // A generic winter window, written first and starting earlier.
    theme_schedule::create(
        tdb.db(),
        "example.test",
        "Winter",
        &json!({ "winter": 1.0 }),
        "2026-12-01T00:00:00Z",
        "2027-02-01T00:00:00Z",
        NOW,
    )
    .await
    .expect("winter");
    // A specific event inside it.
    theme_schedule::create(
        tdb.db(),
        "example.test",
        "Christmas",
        &json!({ "christmas": 5.0 }),
        "2026-12-24T00:00:00Z",
        "2026-12-26T00:00:00Z",
        NOW,
    )
    .await
    .expect("christmas");

    let active = theme_schedule::active_at(tdb.db(), "example.test", "2026-12-25T00:00:00Z")
        .await
        .expect("query")
        .expect("something applies");
    assert_eq!(
        active.label, "Christmas",
        "the more specific intent — the later start — is the one that wins"
    );

    // Outside the event, the winter window is what applies.
    let active = theme_schedule::active_at(tdb.db(), "example.test", "2026-12-10T00:00:00Z")
        .await
        .expect("query")
        .expect("winter covers it");
    assert_eq!(active.label, "Winter");
}

#[tokio::test]
async fn a_revoked_window_stops_applying_and_stays_listed() {
    let tdb = db("revoked").await;
    let id = theme_schedule::create(
        tdb.db(),
        "example.test",
        "Cancelled fest",
        &json!({ "fest": 3.0 }),
        "2026-12-24T00:00:00Z",
        "2026-12-26T00:00:00Z",
        NOW,
    )
    .await
    .expect("create");

    assert!(
        theme_schedule::revoke(tdb.db(), "example.test", &id, "2026-12-01T00:00:00Z")
            .await
            .expect("revoke"),
        "the first revoke changes a row"
    );
    assert!(
        !theme_schedule::revoke(tdb.db(), "example.test", &id, "2026-12-02T00:00:00Z")
            .await
            .expect("revoke again"),
        "a second revoke is a no-op that says so, not an error"
    );

    assert!(
        theme_schedule::active_at(tdb.db(), "example.test", "2026-12-25T00:00:00Z")
            .await
            .expect("query")
            .is_none(),
        "a revoked window applies to nothing, however wide its dates"
    );

    let listed = theme_schedule::list_for_instance(tdb.db(), "example.test")
        .await
        .expect("list");
    assert_eq!(listed.len(), 1, "the row is still there to be read back");
    assert!(listed[0].revoked_at.is_some(), "and says it was called off");
}

#[tokio::test]
async fn another_instance_never_sees_this_one_s_windows() {
    let tdb = db("scoped").await;
    theme_schedule::create(
        tdb.db(),
        "example.test",
        "Christmas",
        &json!({ "christmas": 5.0 }),
        "2026-12-24T00:00:00Z",
        "2026-12-26T00:00:00Z",
        NOW,
    )
    .await
    .expect("create");

    assert!(
        theme_schedule::active_at(tdb.db(), "other.test", "2026-12-25T00:00:00Z")
            .await
            .expect("query")
            .is_none(),
        "an instance's seasonal gravity is not another instance's"
    );
    assert!(theme_schedule::list_for_instance(tdb.db(), "other.test")
        .await
        .expect("list")
        .is_empty());
}

#[tokio::test]
async fn a_backwards_window_is_refused_at_the_store_not_only_at_the_router() {
    let tdb = db("backwards").await;
    let failure = theme_schedule::create(
        tdb.db(),
        "example.test",
        "Impossible",
        &json!({}),
        "2026-12-26T00:00:00Z",
        "2026-12-24T00:00:00Z",
        NOW,
    )
    .await
    .expect_err("refused");
    assert!(
        failure.to_string().contains("not after it starts"),
        "the store refuses with the same sentence the router shows: {failure}"
    );
    assert!(theme_schedule::list_for_instance(tdb.db(), "example.test")
        .await
        .expect("list")
        .is_empty());
}
