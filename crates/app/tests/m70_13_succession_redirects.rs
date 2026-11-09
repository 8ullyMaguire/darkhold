//! The redirect registry, which an operator could write and never read (M70-13 pass 55).
//!
//! §11.11a's registry answers two different questions and only one of them had a door:
//!
//! * **where does this path go now** — `GET /succession/redirects?host=&path=`, resolved by longest matching prefix, and
//!   public because a reader following an old link is not an administrator;
//! * **what have I published** — `redirects_for(host)`, which had no caller at all.
//!
//! The second one is not a convenience. An operator can `PUT` a redirect blindly, and the only way to check their work was to
//! guess paths one at a time — so a duplicate, a typo'd host, or a redirect pointing at an archive that has moved again all
//! stay invisible until a reader complains. A registry nobody can list is a registry nobody can audit.
//!
//! The listing is operator-only: the whole table is instance internals (every host and path this instance has ever answered
//! for), which is not something a stranger should be able to enumerate, however harmless a single path looks.

use axum::http::StatusCode;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::Database;
use serde_json::json;
use test_support::{register, scratch_dir, TestClient, TestDb};

fn build(config: &Config, db: &Database) -> axum::Router {
    server::build_router(AppState::new(config.clone(), db.clone()))
}

/// An account with `role:operator` on its session row. The role is read per request, so no rebuild is needed — but the
/// client must keep the cookie the row belongs to, which registration already set.
async fn operator(db: &Database, config: &Config, email: &str, handle: &str) -> TestClient {
    let mut client = TestClient::new(build(config, db));
    let account = register(&mut client, email, handle).await;
    test_support::exec(
        db,
        "UPDATE sessions SET roles = '[\"role:operator\"]' WHERE account_id = ?",
        "UPDATE sessions SET roles = '[\"role:operator\"]' WHERE account_id = $1::uuid",
        &account,
    )
    .await;
    client
}

#[tokio::test]
async fn an_operator_can_list_the_redirects_the_public_door_resolves() {
    let dir = scratch_dir("succession-redirects");
    let tdb = TestDb::connect_with_dir("m7013-succession-redirects", &dir).await;
    let db: Database = tdb.db().clone();
    let config = Config::development_defaults();

    let mut admin = operator(&db, &config, "redir-admin@example.test", "rediradmin").await;

    // 1. An empty registry answers an empty list, not a 404: "nothing is published" is the answer to a question that has
    //    one, and a 404 here would be indistinguishable from "this door does not exist".
    let (status, body) = admin
        .get("/api/v1/admin/succession/redirects?host=old.example.test")
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["host"], json!("old.example.test"), "{body}");
    assert_eq!(body["redirects"], json!([]), "{body}");

    // 2. Two redirects on one host, written through the door that already existed.
    for (path, to) in [
        ("/works", "https://archive.example.test/works"),
        ("/works/1234", "https://archive.example.test/rescued/1234"),
    ] {
        let (status, body) = admin
            .request(
                "PUT",
                "/api/v1/succession/redirects",
                Some(json!({ "host": "old.example.test", "path": path, "to_url": to })),
            )
            .await;
        assert!(status.is_success(), "{path}: {body}");
    }

    // 3. The listing shows both, and it is the same registry the public resolver reads — asserted by resolving each path
    //    through the public door in the same test, so the list cannot drift from what readers get.
    let (status, body) = admin
        .get("/api/v1/admin/succession/redirects?host=old.example.test")
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let listed: Vec<(String, String)> = body["redirects"]
        .as_array()
        .expect("an array")
        .iter()
        .map(|row| {
            (
                row["path"].as_str().expect("path").to_owned(),
                row["to_url"].as_str().expect("to_url").to_owned(),
            )
        })
        .collect();
    // Longest prefix first: that is the resolution order, so the listing answers the question an operator actually has —
    // which rule wins for a given URL — rather than being alphabetical and silently implying `/works` catches `/works/1234`.
    assert_eq!(
        listed,
        vec![
            (
                "/works/1234".to_owned(),
                "https://archive.example.test/rescued/1234".to_owned()
            ),
            (
                "/works".to_owned(),
                "https://archive.example.test/works".to_owned()
            ),
        ],
        "the list is in resolution order (longest prefix first), and it is what readers resolve: {body}"
    );

    let mut reader = TestClient::new(build(&config, &db));
    let (status, body) = reader
        .get("/api/v1/succession/redirects?host=old.example.test&path=/works/1234")
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["to"],
        json!("https://archive.example.test/rescued/1234"),
        "the longest matching prefix, exactly as listed: {body}"
    );

    // 4. Another host is not this host: the listing is per host, so an operator of one succession cannot read another's.
    let (status, body) = admin
        .get("/api/v1/admin/succession/redirects?host=other.example.test")
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["redirects"], json!([]), "{body}");

    // 5. Gating. A signed-in reader with no standing gets the same answer as a door they cannot see (ADR 0030: 404, not 403,
    //    because a 403 confirms the door exists), and an anonymous caller is refused a session.
    let mut plain = TestClient::new(build(&config, &db));
    register(&mut plain, "redir-plain@example.test", "redirplain").await;
    let (status, body) = plain
        .get("/api/v1/admin/succession/redirects?host=old.example.test")
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a reader with no standing must not be able to enumerate the registry: {body}"
    );

    let mut anonymous = TestClient::new(build(&config, &db));
    let (status, body) = anonymous
        .get("/api/v1/admin/succession/redirects?host=old.example.test")
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
}
