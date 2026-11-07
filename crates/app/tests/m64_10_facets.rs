//! M64-10 — length, status, rating and cadence are first-class facets, and a facet nobody measured says so.
//!
//! The failure this guards is the one the whole row is about: an unmeasured facet and an empty one look identical on a page.
//! A catalogue where `word_count` was never imported must answer "no work here has a known length yet" rather than draw an
//! empty histogram, because the reader's next move — close the tab — is the same in both cases and only one of them is true.
//!
//! Asserts:
//! 1. all four facets describe the same set, and that set is the public catalogue (an unlisted work is in none of them);
//! 2. length buckets land where the words are, and a work with no measured unit counts as `unknown`, not as zero words;
//! 3. a filter that leaves nothing measured produces the sentence, not an empty list;
//! 4. `?facet=` narrows the answer, and an unknown facet name is a 422 rather than a silently empty answer.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use darkhold_app::config::Config;
use darkhold_app::server::{self, set_trust_proxy};
use darkhold_app::state::AppState;
use darkhold_db::DatabaseConfig;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tower::ServiceExt;

const ACCOUNT: &str = "01955000-0000-4000-8000-0000000a0001";
const PSEUD: &str = "01955000-0000-4000-8000-0000000b0001";

const SHORT: &str = "01955000-0000-4000-8000-0000000c0001";
const LONG: &str = "01955000-0000-4000-8000-0000000c0002";
const UNMEASURED: &str = "01955000-0000-4000-8000-0000000c0003";
const HIDDEN: &str = "01955000-0000-4000-8000-0000000c0004";

const TAG: &str = "01955000-0000-4000-8000-0000000d0001";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-m6410-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn config_for(dir: &Path) -> Config {
    let mut config = Config::development_defaults();
    config.storage.root = dir.to_path_buf();
    config.database = DatabaseConfig::new(format!(
        "sqlite://{}/darkhold.sqlite?mode=rwc",
        dir.display()
    ));
    config
}

struct Harness {
    _dir: PathBuf,
    tdb: test_support::TestDb,
    config: Config,
}

impl Harness {
    async fn new(tag: &str) -> Self {
        set_trust_proxy(false);
        let dir = scratch_dir(tag);
        let tdb = test_support::TestDb::connect_with_dir(tag, &dir).await;
        let config = config_for(&dir);
        Self {
            _dir: dir,
            tdb,
            config,
        }
    }

    fn client(&self) -> axum::Router {
        server::build_router(AppState::new(self.config.clone(), self.tdb.db().clone()))
    }

    async fn exec(&self, statement: &str) {
        sqlx::query(statement)
            .execute(self.tdb.db().sqlite_pool().expect("sqlite"))
            .await
            .unwrap_or_else(|error| panic!("{statement}\n{error}"));
    }

    async fn seed(&self) {
        self.exec(&format!(
            "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version)
             VALUES ('{ACCOUNT}','active','facets@example.invalid','adult',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ))
        .await;
        self.exec(&format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at,
                                 updated_at, version)
             VALUES ('{PSEUD}','{ACCOUNT}','facetwriter','Facet Writer','listed',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ))
        .await;
        self.exec(&format!(
            "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at)
             VALUES ('{TAG}','tag','slow burn','slow burn','2026-01-01T00:00:00Z')"
        ))
        .await;

        // Three public works: a short one, a long one, and one whose units carry no revision at all — the imported work whose
        // length nobody measured. Plus an unlisted one, which must appear in no facet.
        for (id, title, rating, visibility, published, updated) in [
            (
                SHORT,
                "A Short One",
                "general",
                "public",
                "2026-01-01",
                "2026-01-08",
            ),
            (
                LONG,
                "A Long One",
                "mature",
                "public",
                "2026-01-01",
                "2026-02-05",
            ),
            (
                UNMEASURED,
                "An Unmeasured One",
                "general",
                "public",
                "2026-01-01",
                "2026-01-02",
            ),
            (
                HIDDEN,
                "An Unlisted One",
                "explicit",
                "unlisted",
                "2026-01-01",
                "2026-01-02",
            ),
        ] {
            self.exec(&format!(
                "INSERT INTO works (id, owner_pseud_id, title, language, rating, visibility, lifecycle,
                                    published_at, created_at, updated_at, version)
                 VALUES ('{id}','{PSEUD}','{title}','en','{rating}','{visibility}','published',
                         '{published}T00:00:00Z','{published}T00:00:00Z','{updated}T00:00:00Z',1)"
            ))
            .await;
        }
        self.exec(&format!(
            "INSERT INTO work_tags (work_id, node_id, prominence, added_at)
             VALUES ('{SHORT}','{TAG}',1,'2026-01-01T00:00:00Z')"
        ))
        .await;

        // Words live on the revision a unit points at, so a measured unit is a unit plus its revision.
        for (unit, work, words) in [
            ("01955000-0000-4000-8000-0000000e0001", SHORT, 500i64),
            ("01955000-0000-4000-8000-0000000e0002", LONG, 12_000i64),
        ] {
            let revision = format!(
                "01955000-0000-4000-8000-0000000f000{}",
                &unit[unit.len() - 1..]
            );
            // The unit first, pointing at nothing; the revision next, because each has a foreign key to the other and only
            // one of them can be first.
            self.exec(&format!(
                "INSERT INTO units (id, work_id, order_key, title, created_at, updated_at, version)
                 VALUES ('{unit}','{work}','a','One','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
            ))
            .await;
            self.exec(&format!(
                "INSERT INTO revisions (id, unit_id, revision_number, document_json, sanitized_html,
                                        plain_text, word_count, created_by_pseud_id, created_at)
                 VALUES ('{revision}','{unit}',1,'{{}}','<p>x</p>','x',{words},'{PSEUD}',
                         '2026-01-01T00:00:00Z')"
            ))
            .await;
            self.exec(&format!(
                "UPDATE units SET current_revision_id = '{revision}' WHERE id = '{unit}'"
            ))
            .await;
        }
        // A unit with no revision is the imported shape: the unit exists, the text was never measured.
        self.exec(&format!(
            "INSERT INTO units (id, work_id, order_key, title, created_at, updated_at, version)
             VALUES ('01955000-0000-4000-8000-0000000e0003','{UNMEASURED}','a','One',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ))
        .await;
    }
}

async fn json_of(app: axum::Router, uri: &str) -> (StatusCode, Value) {
    let response: Response = app
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .expect("request");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

fn facet<'a>(body: &'a Value, name: &str) -> &'a Value {
    body["facets"]
        .as_array()
        .expect("facets array")
        .iter()
        .find(|entry| entry["facet"] == name)
        .unwrap_or_else(|| panic!("facet {name} missing from {body}"))
}

#[tokio::test]
async fn the_four_facets_describe_the_public_catalogue_and_a_missing_measurement_says_so() {
    let harness = Harness::new("all").await;
    harness.seed().await;

    let (status, body) = json_of(harness.client(), "/api/v1/facets").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");

    // Three public works; the unlisted one is in no facet.
    let length = facet(&body, "length");
    assert_eq!(length["known"], 2, "the short and the long work: {length}");
    assert_eq!(
        length["unknown"], 1,
        "the unmeasured work is unknown, not zero words: {length}"
    );
    let buckets = length["buckets"].as_array().expect("buckets");
    assert!(
        buckets
            .iter()
            .any(|bucket| bucket["key"] == "under_1k" && bucket["count"] == 1),
        "500 words is in the lowest band: {length}"
    );
    assert!(
        buckets
            .iter()
            .any(|bucket| bucket["key"] == "5k_20k" && bucket["count"] == 1),
        "12,000 words is in the third band: {length}"
    );
    assert!(
        !buckets.iter().any(|bucket| bucket["key"] == "hidden"),
        "no bucket describes the unlisted work: {length}"
    );
    let sentence = length["when_unknown"]
        .as_str()
        .unwrap_or_default()
        .to_lowercase();
    assert!(
        sentence.contains("no known length"),
        "a partly measured facet names the gap: {length}"
    );

    // Rating and status are fully measured here, so they carry no apology.
    let rating = facet(&body, "rating");
    assert_eq!(rating["unknown"], 0, "{rating}");
    assert!(rating["when_unknown"].is_null(), "{rating}");
    assert!(
        rating["buckets"]
            .as_array()
            .expect("buckets")
            .iter()
            .any(|bucket| bucket["key"] == "mature" && bucket["count"] == 1),
        "the long work is the mature one: {rating}"
    );
    assert_eq!(facet(&body, "status")["known"], 3);
    assert_eq!(facet(&body, "status")["buckets"][0]["key"], "published");

    // Cadence needs words and dates both, so it is measured for the same two works.
    assert_eq!(
        facet(&body, "cadence")["known"],
        2,
        "a work with no words has no cadence: {}",
        facet(&body, "cadence")
    );

    // A tag filter answers about that tag's works only, and the low-volume one is genuinely empty rather than unknown: the
    // single work under it has a measured length.
    let (status, tagged) = json_of(harness.client(), "/api/v1/facets?tag=slow%20burn").await;
    assert_eq!(status, StatusCode::OK, "{tagged}");
    let tagged_length = facet(&tagged, "length");
    assert_eq!(tagged_length["known"], 1, "{tagged_length}");
    assert_eq!(tagged_length["unknown"], 0, "{tagged_length}");

    // The browse surface carries the same facets under the tag it lists: the reader narrows from where they are.
    let (status, browsed) = json_of(harness.client(), "/api/v1/tags/slow%20burn").await;
    assert_eq!(status, StatusCode::OK, "{browsed}");
    assert_eq!(
        facet(&browsed, "length")["known"],
        1,
        "the by-tag page answers about its own works: {browsed}"
    );

    // One facet on request.
    let (status, one) = json_of(harness.client(), "/api/v1/facets?facet=rating").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(one["facets"].as_array().expect("facets").len(), 1);

    let (status, _) = json_of(harness.client(), "/api/v1/facets?facet=popularity").await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "an unknown facet is refused by name, not answered with nothing"
    );
}

#[tokio::test]
async fn a_search_response_carries_the_facets_of_the_catalogue_it_narrowed_and_says_which_set_they_describe(
) {
    // The other half of M64-10: the facets must reach the surface a reader narrows from, and the response must not let anyone
    // believe they describe the twenty hits on screen.
    let harness = Harness::new("search").await;
    harness.seed().await;

    // A rating ceiling of `general` must exclude the mature long work from the facet as well as from the hits; a facet that
    // counted it would offer a `mature` bucket the same search then refuses to fill.
    let (status, body) = json_of(harness.client(), "/api/v1/search?q=one&max_rating=general").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["facets_over"]
            .as_str()
            .unwrap_or_default()
            .contains("not this page of hits"),
        "the response names the set the facets describe: {body}"
    );
    let rating = facet(&body, "rating");
    assert_eq!(
        rating["buckets"].as_array().expect("buckets").len(),
        1,
        "only the general works remain under the ceiling: {rating}"
    );
    assert_eq!(rating["buckets"][0]["key"], "general", "{rating}");

    // The word window is on the per-work sum, so a 12,000-word work survives a 1,000-word floor even though its revisions are
    // individually short — and the unmeasured work (0 words) does not.
    let (status, body) = json_of(harness.client(), "/api/v1/search?q=one&min_words=1000").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let length = facet(&body, "length");
    assert_eq!(
        length["known"], 1,
        "only the long work clears the floor: {length}"
    );
    assert_eq!(
        length["unknown"], 0,
        "the unmeasured work cannot clear a floor: {length}"
    );
}

#[tokio::test]
async fn a_filter_that_measures_nothing_returns_the_sentence_rather_than_an_empty_histogram() {
    let harness = Harness::new("empty").await;
    harness.seed().await;

    // The unmeasured work is the whole set under this title filter.
    let (status, body) =
        json_of(harness.client(), "/api/v1/facets?q=Unmeasured&facet=length").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let length = facet(&body, "length");
    assert_eq!(length["known"], 0, "{length}");
    assert_eq!(length["unknown"], 1, "{length}");
    assert!(
        length["buckets"].as_array().expect("buckets").is_empty(),
        "no invented bucket for a value nobody measured: {length}"
    );
    let sentence = length["when_unknown"]
        .as_str()
        .unwrap_or_default()
        .to_lowercase();
    assert!(
        sentence.contains("nothing to filter on") && sentence.contains("gap in the catalogue"),
        "the answer explains the gap instead of showing an empty shelf: {length}"
    );
}

/// M71-12: the word-count distribution is its own door, with population-equal buckets and the same honesty about gaps.
#[tokio::test]
async fn the_word_histogram_door_serves_a_distribution_over_the_same_filtered_set() {
    let harness = Harness::new("words").await;
    harness.seed().await;

    let (status, body) = json_of(harness.client(), "/api/v1/facets/words?buckets=5").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["unit"], "words",
        "the chart says what it counts: {body}"
    );
    assert_eq!(body["measured"], 2, "the short and the long work: {body}");
    assert_eq!(
        body["unknown"], 1,
        "the unmeasured work is a gap in the chart, not a bar at zero: {body}"
    );
    let buckets = body["buckets"].as_array().expect("buckets");
    let counted: i64 = buckets
        .iter()
        .map(|bucket| bucket["count"].as_i64().expect("count"))
        .sum();
    assert_eq!(
        counted, 2,
        "every measured work lands in exactly one bucket: {body}"
    );
    assert_eq!(
        buckets.last().expect("buckets")["high"],
        12_000,
        "the top edge is the longest work in this view: {body}"
    );
    assert!(
        body["when_unknown"]
            .as_str()
            .unwrap_or_default()
            .contains("1 of 3"),
        "the gap is stated in the words the other facets use: {body}"
    );

    // A view where nothing is measured answers with the sentence and no buckets — the page must not draw a flat line at zero.
    let (status, body) = json_of(harness.client(), "/api/v1/facets/words?q=Unmeasured").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["measured"], 0, "{body}");
    assert!(
        body["buckets"].as_array().expect("buckets").is_empty(),
        "no invented buckets: {body}"
    );
    assert!(
        body["when_unknown"]
            .as_str()
            .unwrap_or_default()
            .contains("gap in the catalogue"),
        "it names the catalogue's gap rather than the reader's empty view: {body}"
    );
}
