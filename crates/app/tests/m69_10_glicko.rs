//! M69-10 — the arena over HTTP, on both ladders.
//!
//! Nothing in the workspace issued a roadmap arena request before this file: `crates/app/tests/milestone_45_roadmap.rs` covers
//! the store, and the door itself — ballot out, ballot in — was unexercised. So these tests are as much about the wiring as
//! about the ranking: a card must actually move *through the route*, and the numbers must be the ones an operator can read
//! back afterwards.
//!
//! Three claims:
//!
//! 1. with `[roadmap] glicko` on, a ballot reports a rating **and** a deviation, and the deviation falls after it is compared;
//! 2. the Elo column still receives a rating on the same 1500-mean scale, so no reader of the board changes meaning;
//! 3. with the switch off the response has no `glicko` block at all — not a zeroed one — and the ladder is named `elo`.

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
        "darkhold-m6910-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn router(tdb: &TestDb, dir: &std::path::Path, glicko: bool) -> Router {
    let mut config = Config::development_defaults();
    config.storage.root = dir.to_path_buf();
    config.roadmap.glicko = glicko;
    // Trust 0 is enough to ballot: this file is about the ladder, not the gate.
    config.roadmap.min_trust = 0;
    server::build_router(AppState::new(config, tdb.db().clone()))
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

    async fn register(&mut self, handle: &str) {
        let (status, body) = self
            .send(
                "POST",
                "/api/v1/auth/register",
                Some(json!({
                    "email": format!("m69-10-{handle}@example.test"),
                    "password": "test-pass-1234",
                    "handle": handle,
                    "display_name": handle,
                    "age_band": "adult",
                })),
            )
            .await;
        assert!(
            status == StatusCode::CREATED || status == StatusCode::OK,
            "register: {body}"
        );
    }
}

/// Four idea cards, written straight to the board through the store the seeder uses.
async fn seed_cards(tdb: &TestDb, tag: &str) {
    for index in 0..4 {
        let card = darkhold_db::roadmap::Card {
            id: format!("{tag}-card-{index}"),
            title: format!("Feature {index}"),
            body: format!("Body {index}"),
            category: "general".to_owned(),
            stage: "idea".to_owned(),
            elo_rating: 1500.0,
            matches_played: 0,
            times_best: 0,
            times_worst: 0,
            created_at: "2026-01-01T00:00:00+00:00".to_owned(),
            updated_at: "2026-01-01T00:00:00+00:00".to_owned(),
        };
        darkhold_db::roadmap::upsert_card(tdb.db(), &card)
            .await
            .expect("upsert card");
    }
}

async fn ballot(client: &mut Client) -> (String, Vec<String>) {
    let (status, body) = client.send("GET", "/api/v1/roadmap/arena", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let id = body["ballot_id"].as_str().expect("a ballot").to_owned();
    let cards: Vec<String> = body["cards"]
        .as_array()
        .expect("cards")
        .iter()
        .map(|card| card["id"].as_str().expect("card id").to_owned())
        .collect();
    assert_eq!(cards.len(), 4, "a full ballot: {body}");
    (id, cards)
}

#[tokio::test]
async fn a_glicko_ballot_reports_uncertainty_and_learns_from_being_compared() {
    server::set_trust_proxy(false);
    let dir = scratch_dir("on");
    let tdb = TestDb::connect_with_dir("m69-10-on", &dir).await;
    seed_cards(&tdb, "g-on").await;

    let mut client = Client::new(router(&tdb, &dir, true));
    client.register("glicko-reader").await;

    let (ballot_id, cards) = ballot(&mut client).await;
    // Every card enters adopted at its Elo rating with maximum deviation: the board's numbers predate Glicko-2.
    let (status, before) = client.send("GET", "/api/v1/roadmap/arena", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(before["ladder"], "glicko2", "{before}");

    let (status, body) = client
        .send(
            "POST",
            "/api/v1/roadmap/arena",
            Some(json!({
                "ballot_id": ballot_id,
                "best_id": cards[0],
                "worst_id": cards[3],
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ladder"], "glicko2", "{body}");

    let rated = body["cards"].as_array().expect("rated cards");
    assert_eq!(rated.len(), 4);
    let best = rated
        .iter()
        .find(|row| row["card_id"] == cards[0].as_str())
        .expect("best was rated");
    assert!(
        best["rating"].as_f64().expect("rating") > 1500.0,
        "the best card rises: {best}"
    );
    assert!(
        best["deviation"].as_f64().expect("deviation") < 350.0,
        "and it becomes less uncertain by being compared: {best}"
    );
    assert_eq!(
        best["diverged"], false,
        "four cards at the same rating invert no 300-point gap: {best}"
    );

    let worst = rated
        .iter()
        .find(|row| row["card_id"] == cards[3].as_str())
        .expect("worst was rated");
    assert!(
        worst["rating"].as_f64().expect("rating") < 1500.0,
        "the worst card falls: {worst}"
    );

    // The numbers are stored, and the next ballot sees them: this is what makes "provisional" outlive a restart.
    let stored = darkhold_db::roadmap_glicko::load(tdb.db(), &cards[0])
        .await
        .expect("load")
        .expect("a row for the card that moved");
    assert!(stored.deviation < 350.0, "{stored:?}");

    // The Elo column received the Glicko rating, so the board still orders the way every reader expects.
    let cards_out = darkhold_db::roadmap::list_cards(tdb.db(), None)
        .await
        .expect("board");
    let best_row = cards_out
        .iter()
        .find(|card| card.id == cards[0])
        .expect("best on the board");
    assert!(
        best_row.elo_rating > 1500.0,
        "the board's own ordering column moved with the ladder: {}",
        best_row.elo_rating
    );
    assert!(best_row.matches_played >= 1);
}

#[tokio::test]
async fn with_the_switch_off_the_response_names_elo_and_carries_no_glicko_block() {
    server::set_trust_proxy(false);
    let dir = scratch_dir("off");
    let tdb = TestDb::connect_with_dir("m69-10-off", &dir).await;
    seed_cards(&tdb, "g-off").await;

    let mut client = Client::new(router(&tdb, &dir, false));
    client.register("elo-reader").await;

    let (ballot_id, cards) = ballot(&mut client).await;
    let (status, before) = client.send("GET", "/api/v1/roadmap/arena", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(before["ladder"], "elo", "{before}");
    for card in before["cards"].as_array().expect("cards") {
        assert!(
            card.get("glicko").is_none(),
            "off means the key is absent, not zeroed: {card}"
        );
    }

    let (status, body) = client
        .send(
            "POST",
            "/api/v1/roadmap/arena",
            Some(json!({
                "ballot_id": ballot_id,
                "best_id": cards[0],
                "worst_id": cards[3],
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ladder"], "elo", "{body}");
    assert!(
        body.get("cards").is_none(),
        "the Elo path reports no per-card numbers, exactly as before M69-10: {body}"
    );
    assert!(
        darkhold_db::roadmap_glicko::load_all(tdb.db())
            .await
            .expect("load")
            .is_empty(),
        "the switch off writes no Glicko-2 rows at all"
    );
}
