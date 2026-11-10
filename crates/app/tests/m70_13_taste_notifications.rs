//! The taste-weighted notification queue (§M18 phase 3, migration 0055) over HTTP (M70-13).
//!
//! `taste_notification_queue` had no producer and no drainer: three store functions with a passing store test and no caller
//! anywhere in the application. The migration states the intent in one sentence — *"When a work is published, aligned users
//! are queued for notification. The worker drains this table."* — and neither half existed.
//!
//! The claims:
//!
//! 1. **Off by default, and off means nothing happens.** The module is `default_on: false` (it writes to people who did not
//!    ask for it) and a disabled module queues nobody *and* answers `404 MODULE_UNAVAILABLE` from both doors — not a `403`,
//!    because the surface is not here rather than not for you.
//! 2. **A publish queues the readers inside the band, and only them.** The author is never queued for their own work, and a
//!    reader outside the band is not queued at all.
//! 3. **The cap holds.** A reader holding three unsent notifications is skipped, so an undrained queue cannot become a
//!    backlog that arrives all at once.
//! 4. **The drain delivers into the inbox, and says what it did.** One row per queued notification, with the work's title,
//!    readable at `/notifications` by the reader it was written for.
//! 5. **The reader can see their own queue depth** without being able to read the plans: a count, not the rows.

use std::path::PathBuf;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use serde_json::{json, Value};
use test_support::TestDb;
use tower::ServiceExt;

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-m7013tastenotify-{tag}-{}-{:?}",
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

    fn capture(&mut self, response: &axum::response::Response) {
        for value in response.headers().get_all(header::SET_COOKIE) {
            let Ok(text) = value.to_str() else { continue };
            let Some((pair, _)) = text.split_once(';') else {
                continue;
            };
            let Some((name, value)) = pair.split_once('=') else {
                continue;
            };
            self.cookies.retain(|(k, _)| k != name.trim());
            if !value.trim().is_empty() {
                self.cookies
                    .push((name.trim().to_owned(), value.trim().to_owned()));
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

    async fn get(&mut self, uri: &str) -> (StatusCode, Value) {
        self.send("GET", uri, None).await
    }

    async fn post(&mut self, uri: &str, body: Value) -> (StatusCode, Value) {
        self.send("POST", uri, Some(body)).await
    }

    async fn register(&mut self, handle: &str) -> String {
        let (status, body) = self
            .post(
                "/api/v1/auth/register",
                json!({
                    "email": format!("m7013tn-{handle}@example.test"),
                    "password": "test-pass-1234",
                    "handle": handle,
                    "display_name": handle,
                    "age_band": "adult",
                }),
            )
            .await;
        assert!(
            status == StatusCode::CREATED || status == StatusCode::OK,
            "register {handle}: {body}"
        );
        let (_, me) = self.get("/api/v1/auth/me").await;
        me["account"]["id"]
            .as_str()
            .or_else(|| me["id"].as_str())
            .expect("account id")
            .to_owned()
    }
}

fn config(dir: &std::path::Path, on: bool, operator: Option<String>) -> Config {
    let mut config = Config::development_defaults();
    config.storage.root = dir.to_path_buf();
    if on {
        config
            .modules
            .enabled
            .push("taste_notifications".to_owned());
    }
    if let Some(id) = operator {
        config.administration.operator_account_id = Some(id.parse().expect("account id"));
    }
    config
}

/// Put a reader inside or outside the operator's taste band.
///
/// `list_users_by_centroid_distance` reads `taste_centroid_distance` and skips a reader whose `taste_vector` is `'[]'`, so
/// the fixture writes both through SQL: the vector is what a real reader accumulates, and the distance is what the instance
/// computed for them.
async fn place_reader(tdb: &TestDb, account: &str, distance: f64) {
    let sql = tdb
        .db()
        .sql(
            "UPDATE accounts SET taste_vector = ?, taste_centroid_distance = ? WHERE id = ?",
            // `taste_vector` is **jsonb** in PostgreSQL (SQLite's is TEXT), so the bind needs the cast here rather than a
            // string that happens to parse: 42804 "column is of type jsonb but expression is of type text".
            "UPDATE accounts SET taste_vector = $1::jsonb, taste_centroid_distance = $2 WHERE id = $3::uuid",
        );
    match tdb.db().backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(&sql)
                .bind("[0.5,0.5,0.5,0.5,0.5]")
                .bind(distance)
                .bind(account)
                .execute(tdb.db().sqlite_pool().expect("sqlite"))
                .await
                .expect("place reader");
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(&sql)
                .bind("[0.5,0.5,0.5,0.5,0.5]")
                .bind(distance)
                .bind(account)
                .execute(tdb.db().postgres_pool().expect("pg"))
                .await
                .expect("place reader");
        }
    }
}

async fn queue_rows_for(tdb: &TestDb, work: &str) -> i64 {
    let sql = tdb.db().sql(
        "SELECT COUNT(*) FROM taste_notification_queue WHERE work_id = ?",
        "SELECT COUNT(*)::bigint FROM taste_notification_queue WHERE work_id = $1::uuid",
    );
    match tdb.db().backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar(&sql)
            .bind(work)
            .fetch_one(tdb.db().sqlite_pool().expect("sqlite"))
            .await
            .expect("count"),
        darkhold_db::Backend::Postgres => sqlx::query_scalar(&sql)
            .bind(work)
            .fetch_one(tdb.db().postgres_pool().expect("pg"))
            .await
            .expect("count"),
    }
}

async fn publish(client: &mut Client, title: &str) -> String {
    let (status, body) = client
        .post("/api/v1/works", json!({ "title": title }))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let work = body["id"].as_str().expect("work").to_owned();
    let version = body["version"].as_i64().expect("version");
    let (status, body) = client
        .post(
            &format!("/api/v1/works/{work}/chapters"),
            json!({ "title": "Chapter One" }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let unit = body["id"].as_str().expect("unit").to_owned();
    let unit_version = body["version"].as_i64().expect("unit version");
    let (status, body) = client
        .send(
            "PATCH",
            &format!("/api/v1/chapters/{unit}"),
            Some(json!({
                "expected_version": unit_version,
                "document": { "type": "doc", "content": [
                    { "type": "paragraph", "content": [{ "type": "text", "text": "Words." }] },
                ]},
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = client
        .post(
            &format!("/api/v1/works/{work}/publish"),
            json!({ "expected_version": version, "idempotency_key": format!("tn-{work}") }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    work
}

#[tokio::test]
async fn a_publish_queues_the_aligned_readers_and_the_drain_puts_them_in_their_inboxes() {
    let dir = scratch_dir("queue");
    let tdb = TestDb::connect_with_dir("m7013tastenotify", &dir).await;

    // 0. Off by default: both doors say the module is not here, and a publish queues nobody.
    let mut off = Client::new(server::build_router(AppState::new(
        config(&dir, false, None),
        tdb.db().clone(),
    )));
    let off_reader = off.register("tn-off").await;
    place_reader(&tdb, &off_reader, 0.05).await;
    // The publisher is a DIFFERENT account, because the author is never queued for their own work: with the reader
    // publishing, the disabled-module assertion could never fail for the right reason, and a mutation removing the switch
    // check would pass. (It did, on the first version of this test.)
    let mut off_author = Client::new(server::build_router(AppState::new(
        config(&dir, false, None),
        tdb.db().clone(),
    )));
    off_author.register("tn-off-author").await;
    let off_work = publish(&mut off_author, "Quiet Instance").await;
    assert_eq!(
        queue_rows_for(&tdb, &off_work).await,
        0,
        "a disabled module must not queue anybody"
    );
    let (status, body) = off.get("/api/v1/me/taste-notifications").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(
        body["error"]["code"].as_str(),
        Some("MODULE_UNAVAILABLE"),
        "disabled is 404 MODULE_UNAVAILABLE, never 403: {body}"
    );

    // The off-instance reader was inside the band on purpose (the disabled-module claim needs a reader who WOULD have been
    // queued); it moves outside now so the next count is about the readers this instance has.
    place_reader(&tdb, &off_reader, 0.95).await;

    // Now an instance that runs it, with an operator.
    let mut operator = Client::new(server::build_router(AppState::new(
        config(&dir, true, None),
        tdb.db().clone(),
    )));
    let operator_id = operator.register("tn-operator").await;
    let mut operator = operator.rejoin(server::build_router(AppState::new(
        config(&dir, true, Some(operator_id)),
        tdb.db().clone(),
    )));

    let mut author = Client::new(server::build_router(AppState::new(
        config(&dir, true, None),
        tdb.db().clone(),
    )));
    author.register("tn-author").await;

    let mut aligned_client = Client::new(server::build_router(AppState::new(
        config(&dir, true, None),
        tdb.db().clone(),
    )));
    let aligned = aligned_client.register("tn-aligned").await;
    place_reader(&tdb, &aligned, 0.10).await;

    let mut outsider = Client::new(server::build_router(AppState::new(
        config(&dir, true, None),
        tdb.db().clone(),
    )));
    let unaligned = outsider.register("tn-unaligned").await;
    place_reader(&tdb, &unaligned, 0.90).await;

    // The author is put INSIDE the band on purpose: otherwise the author exclusion would be untested, because a reader with
    // no taste vector is skipped before the exclusion is reached.
    let author_account = {
        let (_, me) = author.get("/api/v1/auth/me").await;
        me["account"]["id"]
            .as_str()
            .or_else(|| me["id"].as_str())
            .expect("account id")
            .to_owned()
    };
    place_reader(&tdb, &author_account, 0.02).await;

    // 2. A publish queues the aligned reader only: not the outsider, not the author.
    let work = publish(&mut author, "Close To Your Taste").await;
    assert_eq!(
        queue_rows_for(&tdb, &work).await,
        1,
        "exactly one reader is inside the band"
    );

    // The author is inside the band and was still not queued for their own work.
    let (_, mine) = author.get("/api/v1/me/taste-notifications").await;
    assert_eq!(
        mine["pending"],
        json!(0),
        "the author is never queued for their own publication: {mine}"
    );

    // 5. The reader sees their own depth, and only a count.
    let (status, body) = aligned_client.get("/api/v1/me/taste-notifications").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["pending"], json!(1), "{body}");
    assert_eq!(body["cap"], json!(3), "{body}");
    assert!(
        body.get("items").is_none(),
        "a queued row is a plan, not a delivered notification: {body}"
    );

    // 3. The cap: fill the reader's queue, then publish again.
    for index in 0..2 {
        let filler = publish(&mut author, &format!("Filler {index}")).await;
        assert_eq!(queue_rows_for(&tdb, &filler).await, 1, "filler queued");
    }
    let (_, body) = aligned_client.get("/api/v1/me/taste-notifications").await;
    assert_eq!(body["pending"], json!(3), "{body}");
    let over_cap = publish(&mut author, "Over The Cap").await;
    assert_eq!(
        queue_rows_for(&tdb, &over_cap).await,
        0,
        "a reader at the cap is skipped, not queued behind it"
    );
    let (_, body) = aligned_client.get("/api/v1/me/taste-notifications").await;
    assert_eq!(body["pending"], json!(3), "{body}");

    // 4. Only the operator drains, and the delivery lands in the reader's inbox.
    let (status, body) = author
        .post(
            "/api/v1/admin/taste-notifications/drain",
            json!({ "limit": 10 }),
        )
        .await;
    // 404, not 403: `admin_scope::require_estate_hidden` hides the operator surface from anyone who does not hold the
    // estate claim (ADR 0030). A drain is a send, so "forbidden" would confirm the endpoint exists and the sender does.
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    let (status, body) = operator
        .post(
            "/api/v1/admin/taste-notifications/drain",
            json!({ "limit": 10 }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["drained"], json!(3), "{body}");
    assert_eq!(body["delivered"], json!(3), "{body}");
    assert_eq!(body["failed"], json!(0), "{body}");

    let (status, body) = aligned_client.get("/api/v1/notifications").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let items = body["items"].as_array().expect("items");
    let taste: Vec<&Value> = items
        .iter()
        .filter(|item| item["kind"] == json!("taste_suggestion"))
        .collect();
    assert_eq!(
        taste.len(),
        3,
        "one inbox row per drained notification: {body}"
    );
    assert!(
        taste.iter().any(|item| item["title"]
            .as_str()
            .unwrap_or("")
            .contains("Close To Your Taste")),
        "the notification names the work it is about: {body}"
    );

    // The queue is empty afterwards, so a second drain does nothing rather than repeating the inbox rows.
    let (_, body) = aligned_client.get("/api/v1/me/taste-notifications").await;
    assert_eq!(body["pending"], json!(0), "{body}");
    let (status, body) = operator
        .post(
            "/api/v1/admin/taste-notifications/drain",
            json!({ "limit": 10 }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["drained"], json!(0), "{body}");
}

impl Client {
    fn rejoin(&self, app: Router) -> Self {
        Self {
            app,
            cookies: self.cookies.clone(),
        }
    }
}
