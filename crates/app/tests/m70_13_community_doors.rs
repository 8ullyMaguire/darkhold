//! Community: a comment's permalink, one account's presence, a group's role for the viewer, a topic's category, and a
//! mute that actually suppresses (M70-13).
//!
//! Five store functions with no caller, and one of them was not a missing surface but a missing **behaviour**:
//! `record_mentions` checked blocks and never checked mutes, so a reader who muted somebody still received mention
//! notifications from them — the one notification they had explicitly asked the instance not to send.
//!
//! The claims:
//!
//! 1. **A comment's id is an address.** `GET /comments/{id}` answers without a session, because a notification links to it.
//! 2. **A deleted comment is a tombstone, not a 404.** The soft delete blanks the body to `[deleted]`; answering "no such
//!    comment" would rewrite history for everybody who replied to it.
//! 3. **One person's presence obeys the stream's rules** — a muted or blocked account is absent (404), a person who turned
//!    presence off answers "off" with no `last_seen_at`, and an account that has never polled is 404.
//! 4. **A mute suppresses mention notifications** from the muted author, and only for the muter.
//! 5. **A group tells the viewer their own role**, so a client does not have to guess which buttons to show.
//! 6. **The topic writer refuses a category that does not exist**, at the writer, so no future caller can leave a dangling
//!    topic behind.

use axum::http::StatusCode;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::{Backend, Database};
use test_support::{active_pseud_id, id, register, scratch_dir, TestClient, TestDb};

fn build(config: &Config, db: &Database) -> axum::Router {
    server::build_router(AppState::new(config.clone(), db.clone()))
}

async fn account_id_of(client: &mut TestClient) -> String {
    let (status, me) = client.get("/api/v1/auth/me").await;
    assert_eq!(status, StatusCode::OK, "{me}");
    me["account"]["id"].as_str().unwrap_or_default().to_owned()
}

async fn exec(tdb: &TestDb, sqlite: &str, postgres: &str, binds: &[&str]) {
    match tdb.db().backend() {
        Backend::Sqlite => {
            let mut q = sqlx::query(sqlite);
            for b in binds {
                q = q.bind(*b);
            }
            q.execute(tdb.db().sqlite_pool().expect("sqlite"))
                .await
                .unwrap_or_else(|error| panic!("seed row failed: {error}\nSQL: {sqlite}"));
        }
        Backend::Postgres => {
            let mut q = sqlx::query(postgres);
            for b in binds {
                q = q.bind(*b);
            }
            q.execute(tdb.db().postgres_pool().expect("postgres"))
                .await
                .unwrap_or_else(|error| panic!("seed row failed: {error}\nSQL: {postgres}"));
        }
    }
}

/// How many mention rows this pseud has. Written per engine rather than through `count_by`, because that helper binds a
/// `&str` and PostgreSQL will not compare a `uuid` column to `text` without a cast.
async fn mention_count(tdb: &TestDb, mentioned_pseud: &str) -> i64 {
    match tdb.db().backend() {
        Backend::Sqlite => {
            sqlx::query_scalar("SELECT COUNT(*) FROM mention_events WHERE mentioned_pseud = ?")
                .bind(mentioned_pseud)
                .fetch_one(tdb.db().sqlite_pool().expect("sqlite"))
                .await
                .expect("count mentions")
        }
        Backend::Postgres => {
            sqlx::query_scalar("SELECT COUNT(*) FROM mention_events WHERE mentioned_pseud = $1::uuid")
                .bind(mentioned_pseud)
                .fetch_one(tdb.db().postgres_pool().expect("postgres"))
                .await
                .expect("count mentions")
        }
    }
}

#[tokio::test]
async fn a_permalink_a_presence_read_a_group_role_a_category_and_a_mute_that_bites() {
    let dir = scratch_dir("community-doors");
    let tdb = TestDb::connect_with_dir("m7013-community", &dir).await;
    let db: Database = tdb.db().clone();
    let config = Config::development_defaults();

    let mut a = TestClient::new(build(&config, &db));
    register(&mut a, "com-a@example.test", "com-a").await;
    let a_id = account_id_of(&mut a).await;
    let a_pseud = active_pseud_id(&mut a, &a_id).await.expect("a pseud");

    let mut b = TestClient::new(build(&config, &db));
    register(&mut b, "com-b@example.test", "com-b").await;
    let b_id = account_id_of(&mut b).await;
    let b_pseud = active_pseud_id(&mut b, &b_id).await.expect("b pseud");

    let mut c = TestClient::new(build(&config, &db));
    register(&mut c, "com-c@example.test", "com-c").await;
    let c_id = account_id_of(&mut c).await;
    let c_pseud = active_pseud_id(&mut c, &c_id).await.expect("c pseud");

    // A work to comment on.
    let work = id("com-work");
    exec(
        &tdb,
        "INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at) VALUES (?, ?, 'A Work', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        "INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at) VALUES ($1::uuid, $2::uuid, 'A Work', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        &[&work, &a_pseud],
    )
    .await;

    // 4a. A mutes B. B then mentions A and C in one comment: A must hear nothing, C must hear it.
    let (status, _) = a
        .post("/api/v1/me/mutes", serde_json::json!({"muted": b_id}))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, posted) = b
        .post(
            format!("/api/v1/works/{work}/comments"),
            serde_json::json!({"body": "hey @com-a and @com-c, look at this"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{posted}");
    let comment_id = posted["id"].as_str().expect("comment id").to_owned();

    assert_eq!(
        mention_count(&tdb, &a_pseud).await,
        0,
        "a mute suppresses the notification from the muted author"
    );
    assert_eq!(
        mention_count(&tdb, &c_pseud).await,
        1,
        "the same comment still notifies somebody who did not mute its author"
    );

    // 1. The permalink answers without a session.
    let mut anonymous = TestClient::new(build(&config, &db));
    let (status, comment) = anonymous
        .get(format!("/api/v1/comments/{comment_id}"))
        .await;
    assert_eq!(status, StatusCode::OK, "{comment}");
    assert_eq!(
        comment["body"],
        serde_json::json!("hey @com-a and @com-c, look at this"),
        "{comment}"
    );
    assert_eq!(comment["author_pseud"], serde_json::json!(b_pseud), "{comment}");
    assert_eq!(comment["deleted"], serde_json::json!(false), "{comment}");
    assert_eq!(comment["subject_id"], serde_json::json!(work), "{comment}");

    // A comment nobody wrote is 404, and that is a different answer from a deleted one.
    let (status, body) = anonymous
        .get("/api/v1/comments/00000000-0000-4000-8000-000000000000")
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // 2. After the author deletes it, the permalink is a tombstone rather than a hole.
    let (status, _) = b
        .post(
            format!("/api/v1/comments/{comment_id}/delete"),
            serde_json::json!({}),
        )
        .await;
    assert!(status.is_success(), "delete answered {status}");
    let (status, gone) = anonymous
        .get(format!("/api/v1/comments/{comment_id}"))
        .await;
    assert_eq!(status, StatusCode::OK, "{gone}");
    assert_eq!(gone["deleted"], serde_json::json!(true), "{gone}");
    assert_eq!(gone["body"], serde_json::json!("[deleted]"), "{gone}");

    // 3. Presence. A polls the stream, so A has a row and is active now.
    let (status, _) = a.get("/api/v1/presence/stream").await;
    assert_eq!(status, StatusCode::OK);
    let (status, seen) = c.get(format!("/api/v1/presence/{a_id}")).await;
    assert_eq!(status, StatusCode::OK, "{seen}");
    assert_eq!(seen["active_now"], serde_json::json!(true), "{seen}");
    assert!(seen["last_seen_at"].as_str().is_some(), "{seen}");

    // Somebody who never polled has no presence at all.
    let (status, body) = c
        .get("/api/v1/presence/00000000-0000-4000-8000-000000000000")
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // A turns presence off. To everybody else A is then **absent**, exactly as the stream leaves them out: one rule on two
    // surfaces. Answering "offline" would still confirm the account exists and is a presence-bearing thing, which is the
    // disclosure the opt-out is for.
    let (status, _) = a
        .put("/api/v1/me/presence", serde_json::json!({"enabled": false}))
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, hidden) = c.get(format!("/api/v1/presence/{a_id}")).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "an opt-out hides you from the stream and from the single read alike: {hidden}"
    );
    // ... and A still sees their own row, withholding the last-seen time: turning it off hides you from others, not from
    // yourself, and a self-read that returned 404 would look like the preference broke.
    let (status, own) = a.get(format!("/api/v1/presence/{a_id}")).await;
    assert_eq!(status, StatusCode::OK, "{own}");
    assert_eq!(own["enabled"], serde_json::json!(false), "{own}");
    assert_eq!(own["active_now"], serde_json::json!(false), "{own}");
    assert!(
        own["last_seen_at"].is_null(),
        "an opt-out that still reports when you were last here is not an opt-out: {own}"
    );

    // B mutes A, so on B's screen A is absent rather than greyed out.
    let (status, _) = b
        .post("/api/v1/me/mutes", serde_json::json!({"muted": a_id}))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = b.get(format!("/api/v1/presence/{a_id}")).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a muted account should not learn they are online because you looked: {body}"
    );

    // 5. A group tells the viewer their own role.
    let (status, created) = a
        .post("/api/v1/groups", serde_json::json!({"name": "A Group", "privacy": "public"}))
        .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let group = created["id"].as_str().expect("group id").to_owned();
    let (status, _) = c
        .post(format!("/api/v1/groups/{group}/join"), serde_json::json!({}))
        .await;
    assert!(status.is_success(), "join answered {status}");

    let (status, as_member) = c.get(format!("/api/v1/groups/{group}")).await;
    assert_eq!(status, StatusCode::OK, "{as_member}");
    assert_eq!(
        as_member["viewer_role"],
        serde_json::json!("member"),
        "a member sees their own role: {as_member}"
    );
    let (status, as_outsider) = a.get(format!("/api/v1/groups/{group}")).await;
    assert_eq!(status, StatusCode::OK, "{as_outsider}");
    assert!(
        as_outsider["viewer_role"].is_null(),
        "the creator did not join, and null is not the same as \"no access\": {as_outsider}"
    );

    // 6. The topic writer refuses a category that does not exist, whatever route asks.
    let refused = darkhold_db::community::create_topic_with_mode(
        &db,
        "no-such-category",
        &a_pseud,
        "A Topic",
        "normal",
    )
    .await;
    assert!(
        refused.is_err(),
        "a topic pointing at nothing is refused at the writer, not only at the one route that checks"
    );
}
