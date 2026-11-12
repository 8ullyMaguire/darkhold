//! M69-15, the door — a reader on probation may read and rate, and may not publish.
//!
//! The rule module proves the boundaries; this file proves the seam: that `decide()` is reached by a real request, that the
//! switch reaches it, that the refusal is actionable, and that a reader who meets the floor is let through without a
//! restart. One door is guarded on purpose — `POST /works`, where "may not publish yet" is the point of a taste gate — and
//! the rest are named as the remaining half in the ledger rather than left to look guarded.
//!
//! Three cases:
//!
//! 1. **Off by default.** An untouched instance publishes exactly as before and `/auth/membership` says `gate_off`.
//! 2. **On, and the newcomer is refused with numbers.** The refusal names how many ratings are missing and how many days
//!    the window has left, because "access denied" teaches a reader nothing.
//! 3. **On, and the floor opens the door.** The same reader, given ratings, publishes without any other change — which is
//!    also the proof that the gate is about taste rather than about waiting out a clock.

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server::{self, set_trust_proxy};
use darkhold_app::state::AppState;
use darkhold_db::DatabaseConfig;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tower::ServiceExt;

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-m69-15-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn config_for(dir: &Path, gate: bool) -> Config {
    let mut config = Config::development_defaults();
    config.storage.root = dir.to_path_buf();
    config.database = match std::env::var("DARKHOLD_TEST_PG_URL") {
        Ok(url) if !url.is_empty() => DatabaseConfig::new(url),
        _ => DatabaseConfig::new(format!(
            "sqlite://{}/darkhold.sqlite?mode=rwc",
            dir.display()
        )),
    };
    config.governance.membership_probation = gate;
    // Every account in a test database is above a threshold of one, so the gate applies to the population the test built
    // rather than to a number it has to inflate with fake accounts.
    config.governance.membership_population_threshold = 1;
    config.governance.membership_probation_days = 14;
    config.governance.membership_grace_days = 30;
    config.governance.membership_floor_base = 3;
    config.governance.membership_floor_per_thousand = 2;
    config
}

const PASSWORD: &str = "test-pass-1234";

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
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json");
        if !self.cookies.is_empty() {
            let joined = self
                .cookies
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("; ");
            builder = builder.header(header::COOKIE, joined);
        }
        if let Some(token) = self
            .cookies
            .iter()
            .find(|(k, _)| k == "darkhold_csrf")
            .map(|(_, v)| v.to_owned())
        {
            builder = builder.header("x-csrf-token", token);
        }
        let body = match body {
            Some(value) => Body::from(value.to_string()),
            None => Body::empty(),
        };
        let response = self
            .app
            .clone()
            .oneshot(builder.body(body).unwrap())
            .await
            .unwrap();
        let status = response.status();
        self.capture(&response);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn post(&mut self, uri: &str, body: Value) -> (StatusCode, Value) {
        self.send("POST", uri, Some(body)).await
    }

    async fn get(&mut self, uri: &str) -> (StatusCode, Value) {
        self.send("GET", uri, None).await
    }
}

struct Harness {
    _dir: PathBuf,
    tdb: test_support::TestDb,
    config: Config,
}

impl Harness {
    async fn new(tag: &str, gate: bool) -> Self {
        set_trust_proxy(false);
        let dir = scratch_dir(tag);
        let tdb = test_support::TestDb::connect_with_dir(tag, &dir).await;
        let config = config_for(&dir, gate);
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

    /// A registered reader with `ratings` taste samples, and the pseud it acts as.
    async fn reader(&self, email: &str, handle: &str) -> (Client, String, String) {
        let mut client = self.client();
        let (status, registered) = client
            .post(
                "/api/v1/auth/register",
                json!({
                    "email": email,
                    "password": PASSWORD,
                    "handle": handle,
                    "display_name": handle,
                    "age_band": "adult",
                }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{registered}");
        let account = registered["account"]["id"].as_str().expect("id").to_owned();
        let (_, me) = client.get("/api/v1/auth/me").await;
        let pseud = me["pseuds"][0]["id"].as_str().expect("pseud").to_owned();
        (client, account, pseud)
    }

    /// `count` ratings by `account`, on works it publishes itself, so the samples are real rows.
    async fn rate(&self, account: &str, pseud: &str, count: i64) {
        use darkhold_db::Backend;
        let at = "2026-01-01T00:00:00Z";
        for index in 0..count {
            let work = uuid::Uuid::new_v4().to_string();
            let rating = uuid::Uuid::new_v4().to_string();
            match self.tdb.db().backend() {
                Backend::Sqlite => {
                    sqlx::query(
                        "INSERT INTO works (id, owner_pseud_id, title, lifecycle, created_at, updated_at, published_at)
                         VALUES (?1, ?2, 'Sample', 'published', ?3, ?3, ?3)",
                    )
                    .bind(&work)
                    .bind(pseud)
                    .bind(at)
                    .execute(self.tdb.db().sqlite_pool().expect("sqlite"))
                    .await
                    .expect("sample work");
                    sqlx::query(
                        "INSERT INTO rating (id, account_id, pseud_id, work_id, stars, is_public, created_at, updated_at)
                         VALUES (?1, ?2, ?3, ?4, 4, 1, ?5, ?5)",
                    )
                    .bind(&rating)
                    .bind(account)
                    .bind(pseud)
                    .bind(&work)
                    .bind(at)
                    .execute(self.tdb.db().sqlite_pool().expect("sqlite"))
                    .await
                    .expect("sample rating");
                }
                Backend::Postgres => {
                    sqlx::query(
                        "INSERT INTO works (id, owner_pseud_id, title, lifecycle, created_at, updated_at, published_at)
                         VALUES ($1::uuid, $2::uuid, 'Sample', 'published', $3::timestamptz, $3::timestamptz, $3::timestamptz)",
                    )
                    .bind(&work)
                    .bind(pseud)
                    .bind(at)
                    .execute(self.tdb.db().postgres_pool().expect("postgres"))
                    .await
                    .expect("sample work");
                    sqlx::query(
                        "INSERT INTO rating (id, account_id, pseud_id, work_id, stars, is_public, created_at, updated_at)
                         VALUES ($1::uuid, $2::uuid, $3::uuid, $4::uuid, 4, TRUE, $5::timestamptz, $5::timestamptz)",
                    )
                    .bind(&rating)
                    .bind(account)
                    .bind(pseud)
                    .bind(&work)
                    .bind(at)
                    .execute(self.tdb.db().postgres_pool().expect("postgres"))
                    .await
                    .expect("sample rating");
                }
            }
            let _ = index;
        }
    }
}

#[tokio::test]
async fn with_the_gate_off_a_reader_publishes_as_before() {
    let harness = Harness::new("off", false).await;
    let (mut reader, _, _) = harness.reader("off@example.test", "Off").await;

    let (status, body) = reader
        .post("/api/v1/works", json!({ "title": "First" }))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let (status, standing) = reader.get("/api/v1/auth/membership").await;
    assert_eq!(status, StatusCode::OK, "{standing}");
    assert_eq!(standing["standing"], json!("full"), "{standing}");
    assert_eq!(
        standing["because"],
        json!("gate_off"),
        "the default is off, and the reader can see that it is: {standing}"
    );
}

#[tokio::test]
async fn on_probation_a_reader_is_refused_with_the_numbers_that_finish_it() {
    let harness = Harness::new("probation", true).await;
    let (mut reader, _, _) = harness.reader("probation@example.test", "Probation").await;

    let (status, refused) = reader
        .post("/api/v1/works", json!({ "title": "Too soon" }))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    let detail = refused["error"]["field_errors"]["membership"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(
        detail.contains("3 more work(s)"),
        "the refusal says how many ratings are missing: {refused}"
    );

    let (_, standing) = reader.get("/api/v1/auth/membership").await;
    assert_eq!(standing["standing"], json!("probation"), "{standing}");
    assert_eq!(standing["samples_needed"], json!(3), "{standing}");
    assert_eq!(standing["days_left"], json!(14), "{standing}");
}

#[tokio::test]
async fn meeting_the_floor_opens_the_door_without_a_restart() {
    let harness = Harness::new("graduate", true).await;
    let (mut reader, account, pseud) = harness.reader("graduate@example.test", "Graduate").await;

    let (status, _) = reader
        .post("/api/v1/works", json!({ "title": "Too soon" }))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    harness.rate(&account, &pseud, 3).await;

    let (status, published) = reader
        .post("/api/v1/works", json!({ "title": "Earned" }))
        .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "the floor was met, so publishing is allowed: {published}"
    );
    let (_, standing) = reader.get("/api/v1/auth/membership").await;
    assert_eq!(standing["standing"], json!("full"), "{standing}");
    assert_eq!(standing["because"], json!("graduated"), "{standing}");
}

#[tokio::test]
async fn an_account_older_than_the_cutoff_is_grandfathered() {
    let mut harness = Harness::new("grandfathered", true).await;
    // The cutoff is tomorrow, so every account in this database predates the gate.
    let tomorrow = (time::OffsetDateTime::now_utc() + time::Duration::days(1))
        .format(&time::format_description::well_known::Rfc3339)
        .expect("format");
    harness.config.governance.membership_grandfather_before = Some(tomorrow);
    let (mut reader, _, _) = harness.reader("old@example.test", "Old").await;

    let (status, body) = reader
        .post("/api/v1/works", json!({ "title": "Arrived first" }))
        .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "an account that predates the gate is not punished for it: {body}"
    );
    let (_, standing) = reader.get("/api/v1/auth/membership").await;
    assert_eq!(standing["because"], json!("grandfathered"), "{standing}");
}

#[tokio::test]
async fn a_comment_is_a_public_act_and_the_gate_covers_it() {
    let harness = Harness::new("comment", true).await;
    // The author is a full member because they met the floor, so their work is what the reader below comments on.
    let (mut author, author_account, author_pseud) =
        harness.reader("author@example.test", "Author").await;
    harness.rate(&author_account, &author_pseud, 3).await;
    let (status, work) = author
        .post("/api/v1/works", json!({ "title": "Something to discuss" }))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{work}");
    let work_id = work["id"].as_str().expect("work id").to_owned();

    let (mut reader, _, _) = harness.reader("commenter@example.test", "Commenter").await;
    let (status, refused) = reader
        .post(
            &format!("/api/v1/works/{work_id}/comments"),
            json!({ "body": "too soon" }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a comment is visible to strangers, so it is gated like publishing: {refused}"
    );
    assert!(
        refused["error"]["field_errors"]["membership"].is_string(),
        "and the refusal says what finishes it: {refused}"
    );
}

#[tokio::test]
async fn the_forum_is_gated_too_and_the_room_still_works_for_a_member() {
    let harness = Harness::new("forum", true).await;

    let (mut probation, _, _) = harness.reader("newcomer@example.test", "Newcomer").await;
    let (status, refused) = probation
        .post(
            "/api/v1/forums/general/topics",
            json!({ "title": "Hello", "body": "First post" }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a topic is visible to strangers, so it is gated like a comment: {refused}"
    );
    assert!(
        refused["error"]["field_errors"]["membership"].is_string(),
        "{refused}"
    );

    // A member who met the floor posts in the same room, so the refusal above is the gate and not the category.
    let (mut member, account, pseud) = harness.reader("member@example.test", "Member").await;
    harness.rate(&account, &pseud, 3).await;
    let (status, posted) = member
        .post(
            "/api/v1/forums/general/topics",
            json!({ "title": "Hello", "body": "First post" }),
        )
        .await;
    assert!(
        status == StatusCode::CREATED || status == StatusCode::OK,
        "a graduated reader may open a topic in a category with min_trust 0: {posted}"
    );
}
