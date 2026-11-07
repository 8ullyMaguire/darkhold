//! M71-12: the word-count distribution as a real histogram.
//!
//! What this file pins, and why each half matters:
//!
//! 1. **Buckets are population-equal, not width-equal.** On a skewed catalogue equal-width bands put nine of ten buckets in the tail and
//!    hold nobody; the property asserted is that each bucket holds roughly a tenth of the *works*, which is the question a reader has.
//! 2. **The buckets partition the measured set exactly once.** Ties are the interesting case: a catalogue where half the works are 5,000
//!    words must still add up, and the assertion is `sum(counts) == measured`, not "the chart looked right".
//! 3. **Unmeasured works are excluded and reported.** A histogram that counts a null word count as zero draws a bar at the bottom that
//!    says "short" about works nobody measured.
//! 4. **The filters narrow the population**, so the chart describes the same set the page above it shows.

use darkhold_db::facets::{self, FacetFilter};
use darkhold_db::{Backend, Database, DatabaseConfig};

const T0: &str = "2026-01-01T00:00:00Z";
const ACCOUNT: &str = "01955c00-0000-4000-8000-00000000b000";
const PSEUD: &str = "01955c00-0000-4000-8000-00000000b001";
const NARUTO: &str = "01955c00-0000-4000-8000-00000000b010";
const WORM: &str = "01955c00-0000-4000-8000-00000000b011";

fn scratch_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-hw-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

async fn connect(tag: &str) -> Database {
    let dir = scratch_dir(tag);
    let db = match std::env::var("DARKHOLD_TEST_PG_URL") {
        Ok(admin_url) if !admin_url.is_empty() => {
            static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let name = format!(
                "lh_test_hw_{}_{}",
                std::process::id(),
                COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            );
            test_support::empty_postgres(&admin_url, &name)
                .await
                .expect("an empty PostgreSQL database")
        }
        _ => {
            let config =
                DatabaseConfig::new(format!("sqlite://{}/hw.sqlite?mode=rwc", dir.display()));
            Database::connect(&config).await.expect("connect")
        }
    };
    db.migrate().await.expect("migrate");
    db
}

async fn exec(db: &Database, statement: &str) {
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(statement)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await
                .unwrap_or_else(|error| panic!("{statement}\n{error}"));
        }
        Backend::Postgres => {
            sqlx::query(statement)
                .execute(db.postgres_pool().expect("postgres"))
                .await
                .unwrap_or_else(|error| panic!("{statement}\n{error}"));
        }
    }
}

/// One work with one unit whose current revision carries `words`. `words: None` leaves the revision unmeasured, which is the state most
/// imported rows are in today.
async fn work(db: &Database, index: usize, words: Option<i64>, fandom: Option<&str>) {
    let id = format!("01956000-0000-4000-8000-0000000000{index:02}");
    let unit = format!("01957000-0000-4000-8000-0000000000{index:02}");
    let revision = format!("01958000-0000-4000-8000-0000000000{index:02}");
    exec(
        db,
        &format!(
            "INSERT INTO works (id, owner_pseud_id, title, summary, lifecycle, visibility, completion,
                                created_at, updated_at, generated_content_posture)
             VALUES ('{id}','{PSEUD}','Work {index}','','published','public','complete','{T0}','{T0}','forbid')"
        ),
    )
    .await;
    // `revisions.word_count` is NOT NULL, so this schema's "unmeasured" state is a unit with no revision at all — which is exactly
    // what a metadata-only import produces. That state is what the histogram must exclude and report, never count as zero words.
    exec(
        db,
        &format!(
            "INSERT INTO units (id, work_id, order_key, title, current_revision_id, created_at, updated_at)
             VALUES ('{unit}','{id}',1,'One',NULL,'{T0}','{T0}')"
        ),
    )
    .await;
    if let Some(count) = words {
        exec(
            db,
            &format!(
                "INSERT INTO revisions (id, unit_id, revision_number, document_json, sanitized_html, plain_text,
                                        word_count, created_by_pseud_id, created_at)
                 VALUES ('{revision}','{unit}',1,'{{}}','<p>body</p>','body',{count},'{PSEUD}','{T0}')"
            ),
        )
        .await;
        exec(
            db,
            &format!("UPDATE units SET current_revision_id = '{revision}' WHERE id = '{unit}'"),
        )
        .await;
    }
    if let Some(name) = fandom {
        let node = if name == "Naruto" { NARUTO } else { WORM };
        exec(
            db,
            &format!(
                "INSERT INTO work_tags (work_id, node_id, prominence, added_at)
                 VALUES ('{id}','{node}',1,'{T0}')"
            ),
        )
        .await;
    }
}

async fn seed(db: &Database) {
    exec(
        db,
        &format!(
            "INSERT INTO accounts (id, email, status, age_state, created_at, updated_at, permission_statement)
             VALUES ('{ACCOUNT}','hw@example.invalid','active','adult','{T0}','{T0}','minimal')"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at)
             VALUES ('{PSEUD}','{ACCOUNT}','hww','hww','{T0}','{T0}')"
        ),
    )
    .await;
    for (id, name) in [(NARUTO, "Naruto"), (WORM, "Worm")] {
        exec(
            db,
            &format!(
                "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at, review_status, signal_count)
                 VALUES ('{id}','fandom','{name}','{}','{T0}','confirmed',0)",
                name.to_lowercase()
            ),
        )
        .await;
    }
    // Ten measured works spread over three orders of magnitude, plus two unmeasured ones — the shape of the real catalogue.
    for (index, words) in [
        1_000, 2_500, 4_000, 8_000, 12_000, 20_000, 45_000, 90_000, 200_000, 900_000,
    ]
    .into_iter()
    .enumerate()
    {
        work(
            db,
            index + 1,
            Some(words),
            Some(if index % 2 == 0 { "Naruto" } else { "Worm" }),
        )
        .await;
    }
    work(db, 11, None, Some("Naruto")).await;
    work(db, 12, None, Some("Worm")).await;
}

/// Ten buckets, each holding about a tenth of the measured works, edges included.
#[tokio::test]
async fn the_histogram_is_population_equal_and_partitions_the_measured_set() {
    let db = connect("shape").await;
    seed(&db).await;

    let histogram = facets::word_histogram(&db, &FacetFilter::default(), 10)
        .await
        .expect("histogram");
    assert_eq!(histogram.total, 12);
    assert_eq!(histogram.measured, 10);
    assert_eq!(histogram.unknown, 2);
    assert!(
        histogram
            .when_unknown
            .as_deref()
            .is_some_and(|sentence| sentence.contains("2 of 12")),
        "the gap is stated, not charted: {:?}",
        histogram.when_unknown
    );

    let counted: i64 = histogram.buckets.iter().map(|bucket| bucket.count).sum();
    assert_eq!(
        counted, histogram.measured,
        "every measured work lands once"
    );
    // Population-equal: no bucket holds more than a third of the set on ten evenly spread works.
    for bucket in &histogram.buckets {
        assert!(
            bucket.count <= 3,
            "bucket {} holds {} of 10, which is not a tenth-ish: {:?}",
            bucket.index,
            bucket.count,
            histogram.buckets
        );
    }
    // Contiguous and ordered: each bucket starts where the previous one ended or later, and the last reaches the maximum.
    for pair in histogram.buckets.windows(2) {
        assert!(
            pair[0].low <= pair[1].low,
            "ordered: {:?}",
            histogram.buckets
        );
    }
    // Indices are the bars' slots: after empty buckets are dropped they run 0..n with no gaps, which is what a chart draws.
    let indices: Vec<usize> = histogram
        .buckets
        .iter()
        .map(|bucket| bucket.index)
        .collect();
    assert_eq!(
        indices,
        (0..histogram.buckets.len()).collect::<Vec<_>>(),
        "no gaps in the chart: {:?}",
        histogram.buckets
    );
    let last = histogram.buckets.last().expect("buckets");
    assert_eq!(last.high, 900_000, "the top edge is the longest work");
    assert_eq!(
        histogram.buckets[0].low, 1_000,
        "the bottom edge is the shortest"
    );

    // Ties: twelve works of the same length still partition, and the empty buckets are dropped rather than drawn.
    let db = connect("ties").await;
    seed(&db).await;
    exec(&db, "UPDATE revisions SET word_count = 5000").await;
    let tied = facets::word_histogram(&db, &FacetFilter::default(), 10)
        .await
        .expect("histogram");
    assert_eq!(
        tied.measured, 10,
        "the ten measured works, all now the same length"
    );
    assert_eq!(
        tied.buckets.len(),
        1,
        "one populated bucket: {:?}",
        tied.buckets
    );
    assert_eq!(tied.buckets[0].count, 10);
    assert_eq!(tied.buckets[0].low, 5_000);
    assert_eq!(tied.buckets[0].high, 5_000);
}

/// The filter narrows the chart with the page: a fandom's histogram is that fandom's works, not the catalogue's.
#[tokio::test]
async fn the_histogram_describes_the_filtered_set() {
    let db = connect("filtered").await;
    seed(&db).await;

    let filter = FacetFilter {
        tag: Some("Naruto".to_owned()),
        tag_kind: Some("fandom".to_owned()),
        ..FacetFilter::default()
    };
    let histogram = facets::word_histogram(&db, &filter, 5)
        .await
        .expect("histogram");
    assert_eq!(
        histogram.total, 6,
        "six Naruto works, one of them unmeasured"
    );
    assert_eq!(histogram.measured, 5);
    assert_eq!(
        histogram.buckets.iter().map(|b| b.count).sum::<i64>(),
        5,
        "the counts describe the filtered set: {:?}",
        histogram.buckets
    );
    assert!(
        histogram.buckets.iter().all(|b| b.high <= 900_000),
        "no Naruto work is longer than the longest we seeded"
    );

    // An empty set yields no buckets and no complaint — nothing to chart is not a failure.
    let empty = facets::word_histogram(
        &db,
        &FacetFilter {
            tag: Some("Fandom Nobody Tagged".to_owned()),
            tag_kind: Some("fandom".to_owned()),
            ..FacetFilter::default()
        },
        10,
    )
    .await
    .expect("histogram");
    assert_eq!(empty.total, 0);
    assert!(empty.buckets.is_empty());
    assert_eq!(empty.when_unknown, None, "nothing matched is not a gap");
}

/// Nothing measured anywhere: the answer is a sentence, not ten empty bars.
#[tokio::test]
async fn an_unmeasured_catalogue_gets_a_sentence_instead_of_a_chart() {
    let db = connect("unmeasured").await;
    seed(&db).await;
    exec(&db, "UPDATE units SET current_revision_id = NULL").await;

    let histogram = facets::word_histogram(&db, &FacetFilter::default(), 10)
        .await
        .expect("histogram");
    assert_eq!(histogram.measured, 0);
    assert_eq!(histogram.unknown, 12);
    assert!(histogram.buckets.is_empty(), "no bars to draw");
    let sentence = histogram.when_unknown.expect("a sentence");
    assert!(
        sentence.contains("gap in the catalogue"),
        "it says the catalogue is unmeasured rather than pretending it is empty: {sentence}"
    );
}
