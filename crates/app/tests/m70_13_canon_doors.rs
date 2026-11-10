//! §50.2's canon class reaches a reader (M70-13).
//!
//! Three functions in `canon_agnostic.rs` had no caller and the producer had none either, so the feature was complete and
//! invisible: the classifier could run from a test and nothing else.
//!
//! * `stored_canon` — the verdict, now on the work as `GET /works/{id}/canon`, with `state` saying which answer it is:
//!   `canon_agnostic`, `canon_dependent`, `unclassified` (with the reason), or `unmeasured`. Those are four different
//!   claims and a boolean would collapse three of them.
//! * `is_canon_agnostic` / `is_canon_dependent` — §50.2's eligibility and its mirror, answered as two booleans because a
//!   badge and a filter are two client decisions. Both are `false` when nobody has measured the work: an unmeasured work is
//!   offered to nobody as canon-agnostic, which is the conservative direction the module documents.
//! * `measure_work_and_store` — the producer. It now runs when a work is published, which is the moment its text stops
//!   changing, and the operator's `POST /works/{id}/canon/measure` is the backfill. Without either, the canon-blind listing
//!   was empty on a fresh instance in a way that reads as a fact about the corpus.
//!
//! The listing is the labelled, bounded shelf §50.2 asks for — twelve rows, newest first, every row carrying its label, and
//! nothing ranked (there is no score here to outrank a recognised match with).

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

/// Prose the classifier can read: `words` tokens, with `names` mid-sentence proper nouns.
///
/// Terminators matter. `classify` excludes a capital that starts a sentence, so a fixture full of capitals without full
/// stops measures zero unexplained names however suspicious it looks — slot 0 of each four-token group carries no
/// terminator, which is what makes slot 1 the mid-sentence slot.
fn prose(words: usize, names: usize) -> String {
    let mut left = names;
    let mut out = String::new();
    for i in 0..words {
        out.push_str(&match i % 4 {
            0 => format!("word{i} "),
            1 if left > 0 => {
                left -= 1;
                format!("Name{i}. ")
            }
            _ => format!("word{i}. "),
        });
    }
    out
}

/// A draft: created and never published. A work with no text cannot be *published* at all ("an empty work has nothing to
/// read"), so the "nothing measured" case is a draft and the measure door is what asks about it.
async fn draft(client: &mut TestClient, title: &str) -> String {
    let (status, body) = client
        .post("/api/v1/works", json!({ "title": title }))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_str().expect("work").to_owned()
}

/// Create, chapter, write and publish — the road a work actually travels, so the producer is proved where it runs.
async fn publish(client: &mut TestClient, title: &str, text: &str) -> String {
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
        .request(
            "PATCH",
            &format!("/api/v1/chapters/{unit}"),
            Some(json!({
                "expected_version": unit_version,
                "document": { "type": "doc", "content": [
                    { "type": "paragraph", "content": [{ "type": "text", "text": text }] },
                ]},
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = client
        .post(
            &format!("/api/v1/works/{work}/publish"),
            json!({ "expected_version": version, "idempotency_key": format!("canon-{work}") }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    work
}

#[tokio::test]
async fn publishing_measures_the_class_and_the_shelf_carries_it_labelled() {
    let dir = scratch_dir("canon-doors");
    let tdb = TestDb::connect_with_dir("m7013-canon-doors", &dir).await;
    let db: Database = tdb.db().clone();

    let base = Config::development_defaults();
    let mut bootstrap = TestClient::new(build(&base, &db));
    let operator_account = register(&mut bootstrap, "cd-operator@example.test", "cdoperator").await;
    let mut config = base.clone();
    config.administration.operator_account_id = Some(operator_account.parse().expect("account id"));
    let mut operator = TestClient::new(build(&config, &db));
    sign_in_as(
        &mut operator,
        &tdb,
        "cd-operator@example.test",
        "cdoperator",
    )
    .await;

    let mut author = TestClient::new(build(&config, &db));
    register(&mut author, "cd-author@example.test", "cdauthor").await;

    // 1. A draft nobody has measured: the read door answers "unmeasured", not a 404 and not "not agnostic".
    let unmeasured = draft(&mut author, "No Text Yet").await;
    let (status, view) = author
        .get(format!("/api/v1/works/{unmeasured}/canon"))
        .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["state"], json!("unmeasured"), "{view}");
    assert_eq!(view["eligible"], json!(false), "{view}");
    assert_eq!(view["label"], Value::Null, "{view}");

    // 2. Measuring a work with no text stores nothing and says why, rather than storing "unclassified" — writing a row for
    //    a work nobody could measure would turn "we did not look" into "we looked and could not tell".
    let (status, body) = operator
        .post(
            &format!("/api/v1/works/{unmeasured}/canon/measure"),
            json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["state"], json!("unmeasured"), "{body}");
    assert_eq!(body["reason"], json!("no_text"), "{body}");

    // 3. The producer runs on publication: 520 plain words, mid-sentence capital included, is canon-agnostic.
    let agnostic = publish(&mut author, "Readable Alone", &prose(520, 0)).await;
    let (status, view) = author.get(format!("/api/v1/works/{agnostic}/canon")).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(
        view["state"],
        json!("canon_agnostic"),
        "the publish path measures the class, or nothing does: {view}"
    );
    assert_eq!(view["eligible"], json!(true), "{view}");
    assert_eq!(view["assumes_canon"], json!(false), "{view}");
    assert_eq!(
        view["label"],
        json!("you may not know this fandom"),
        "{view}"
    );
    assert!(
        view["measures"]["word_count"].as_i64().unwrap_or(0) >= 500,
        "{view}"
    );

    // 4. The mirror: 120 proper nouns in the same number of words is canon-dependent, and the label is *absent* rather than
    //    the opposite sentence — a work that assumes canon is not offered on this shelf at all.
    let dependent = publish(&mut author, "Names Everywhere", &prose(520, 120)).await;
    let (status, view) = author.get(format!("/api/v1/works/{dependent}/canon")).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["state"], json!("canon_dependent"), "{view}");
    assert_eq!(view["assumes_canon"], json!(true), "{view}");
    assert_eq!(view["eligible"], json!(false), "{view}");
    assert_eq!(view["label"], Value::Null, "{view}");

    // 5. The third state: text that is there but too short to classify is `unclassified` with a reason, and it is neither
    //    eligible nor labelled. This is the state a boolean would have collapsed into "not agnostic".
    let short = publish(&mut author, "Three Words", "Words. Words. Words.").await;
    let (status, view) = author.get(format!("/api/v1/works/{short}/canon")).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["state"], json!("unclassified"), "{view}");
    assert_eq!(view["reason"], json!("too_short"), "{view}");
    assert_eq!(view["eligible"], json!(false), "{view}");
    assert_eq!(view["label"], Value::Null, "{view}");

    // A second agnostic work, so the shelf has more than the page can hold at limit=1 and the limit is not a vacuous
    // assertion on a single row.
    let agnostic_two = publish(&mut author, "Also Readable Alone", &prose(540, 0)).await;

    // 6. The shelf: bounded, labelled, and holding the agnostic works only.
    let (status, shelf) = author.get("/api/v1/discover/canon-blind").await;
    assert_eq!(status, StatusCode::OK, "{shelf}");
    assert_eq!(shelf["state"], json!("ok"), "{shelf}");
    let items = shelf["items"].as_array().expect("items");
    assert!(
        items.len() <= 12,
        "the page is a shelf, not a feed: {}",
        items.len()
    );
    assert_eq!(
        items.len(),
        2,
        "both agnostic works are on the shelf: {shelf}"
    );
    let ids: Vec<&str> = items
        .iter()
        .map(|row| row["work_id"].as_str().expect("work id"))
        .collect();
    assert!(ids.contains(&agnostic.as_str()), "{shelf}");
    assert!(ids.contains(&agnostic_two.as_str()), "{shelf}");
    assert!(
        !ids.contains(&dependent.as_str()),
        "a canon-dependent work is not on the canon-blind shelf: {shelf}"
    );
    for row in items {
        assert_eq!(row["label"], json!("you may not know this fandom"), "{row}");
        assert_eq!(row["state"], json!("canon_agnostic"), "{row}");
    }

    // 7. The limit is a limit, not a suggestion.
    let (status, shelf) = author.get("/api/v1/discover/canon-blind?limit=1").await;
    assert_eq!(status, StatusCode::OK, "{shelf}");
    assert_eq!(
        shelf["items"].as_array().expect("items").len(),
        1,
        "{shelf}"
    );

    // 8. Measuring is an operator's act: an author asking for their own work is answered with the absent page.
    let (status, _) = author
        .post(
            &format!("/api/v1/works/{agnostic}/canon/measure"),
            json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
