//! M63-37's ranking half — `likes_per_view` and `wilson` rank on a view count, against the loaded population.
//!
//! The property is the **denominator's provenance and its absence**:
//!
//! 1. a rate is computed from one archive's own pair (`views` under whichever name that archive uses — `views` or `hits` — falling back
//!    to the imported `library_items.views` column for rows imported before the engagement table existed);
//! 2. a population where no row carries a view count is reported as `views_rows: 0` and `population: N`, so the route can refuse by
//!    name instead of serving every row at zero — and the row that *does* carry one is the only one that scores.

use darkhold_db::catalogue::CatalogueQuery;
use darkhold_db::metrics;
use darkhold_db::{Backend, Database, DatabaseConfig};
use darkhold_domain::quality::Metric;
use darkhold_domain::quality::QualityWeights;

const T0: &str = "2026-01-01T00:00:00Z";
const ACCOUNT: &str = "01955c00-0000-4000-8000-00000000e637";

fn scratch_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-rate-{tag}-{}-{:?}",
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
                "lh_test_rate_{}_{}",
                std::process::id(),
                COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            );
            test_support::empty_postgres(&admin_url, &name)
                .await
                .expect("an empty PostgreSQL database")
        }
        _ => {
            let config =
                DatabaseConfig::new(format!("sqlite://{}/rate.sqlite?mode=rwc", dir.display()));
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

/// One imported item. `views_column` fills `library_items.views` — the old importer's column — and stays NULL for rows that only have
/// a recorded metric, which is the case the join has to handle.
async fn item(
    db: &Database,
    id: &str,
    account: &str,
    key: &str,
    likes: i64,
    views_column: Option<i64>,
) {
    let views = views_column
        .map(|value| value.to_string())
        .unwrap_or_else(|| "NULL".to_owned());
    exec(
        db,
        &format!(
            "INSERT INTO library_items (id, account_id, source_key, source_work_key, title, author_text,
                                        summary, status, source_url, likes, views, provenance_json,
                                        created_at, updated_at, version)
             VALUES ('{id}','{account}','ao3','{key}','{key}','A','','complete',
                     'https://example.invalid/{key}',{likes},{views},'{{}}','{T0}','{T0}',1)"
        ),
    )
    .await;
}

async fn metric(db: &Database, id: &str, name: &str, value: i64) {
    exec(
        db,
        &format!(
            "INSERT INTO library_item_engagement (library_item_id, metric, value, reported_at)
             VALUES ('{id}','{name}',{value},'{T0}')"
        ),
    )
    .await;
}

fn query() -> CatalogueQuery {
    CatalogueQuery {
        limit: Some(50),
        ..CatalogueQuery::default()
    }
}

async fn seed(db: &Database) {
    exec(
        db,
        &format!(
            "INSERT INTO accounts (id, email, status, age_state, created_at, updated_at, permission_statement)
             VALUES ('{ACCOUNT}','rate@example.invalid','active','adult','{T0}','{T0}','minimal')"
        ),
    )
    .await;

    // `big`: 400 kudos over 9,000 hits, recorded under AO3's own word `hits`.
    item(
        db,
        "01956000-0000-4000-8000-000000000001",
        ACCOUNT,
        "big",
        400,
        None,
    )
    .await;
    metric(db, "01956000-0000-4000-8000-000000000001", "hits", 9_000).await;
    metric(db, "01956000-0000-4000-8000-000000000001", "kudos", 400).await;

    // `lucky`: 1 kudos over 10 hits — a 10% rate, and one person clicking.
    item(
        db,
        "01956000-0000-4000-8000-000000000002",
        ACCOUNT,
        "lucky",
        1,
        None,
    )
    .await;
    metric(db, "01956000-0000-4000-8000-000000000002", "hits", 10).await;
    metric(db, "01956000-0000-4000-8000-000000000002", "kudos", 1).await;

    // `legacy`: no engagement rows, but the old importer filled the column.
    item(
        db,
        "01956000-0000-4000-8000-000000000003",
        ACCOUNT,
        "legacy",
        50,
        Some(1_000),
    )
    .await;

    // `silent`: nothing measured at all.
    item(
        db,
        "01956000-0000-4000-8000-000000000004",
        ACCOUNT,
        "silent",
        0,
        None,
    )
    .await;
}

/// The three sources of a view count all reach the ranker, and the rate prefers evidence over a ratio.
#[tokio::test]
async fn a_rate_ranks_from_the_archives_own_views_and_prefers_evidence() {
    let db = connect("rate").await;
    seed(&db).await;

    let page = metrics::ranked_with_weights(&db, &query(), Metric::LikesPerView, 10, 0, &QualityWeights::default())
        .await
        .expect("ranked");
    assert_eq!(page.views_rows, 3, "hits twice and the legacy column once");
    assert_eq!(page.population, 4);

    let order: Vec<&str> = page
        .items
        .iter()
        .map(|item| item.row.source_work_key.as_str())
        .collect();
    // legacy 50/1000 = 5%, big 400/9000 = 4.4%, lucky 1/10 = 10% — the ratio sorts
    // `lucky` first, which is exactly what the point estimate gets wrong.
    let plain = order.clone();
    assert_eq!(plain[0], "lucky", "the ratio is 10%: {plain:?}");
    assert_eq!(plain[1], "legacy", "{plain:?}");
    assert_eq!(plain[2], "big", "{plain:?}");
    assert_eq!(plain[3], "silent", "no views, so it is unmeasured and last");

    // The bound puts `big` ahead of every thin arm: 4.0%, 3.8%, 1.8%.
    let bounded = metrics::ranked_with_weights(&db, &query(), Metric::Wilson, 10, 0, &QualityWeights::default())
        .await
        .expect("ranked");
    let bounded_order: Vec<&str> = bounded
        .items
        .iter()
        .map(|item| item.row.source_work_key.as_str())
        .collect();
    assert_eq!(
        bounded_order[0], "big",
        "the lower bound ranks evidence first: {bounded_order:?}"
    );
    assert!(
        bounded_order
            .iter()
            .position(|key| *key == "lucky")
            .unwrap()
            > bounded_order.iter().position(|key| *key == "big").unwrap(),
        "1/10 must not outrank 400/9,000 on the bound: {bounded_order:?}"
    );
}

/// The population with no views at all is reported as such — the number the route turns into a refusal instead of a page of zeroes.
#[tokio::test]
async fn a_population_with_no_views_says_so_instead_of_scoring_zeroes() {
    let db = connect("noviews").await;
    exec(
        &db,
        &format!(
            "INSERT INTO accounts (id, email, status, age_state, created_at, updated_at, permission_statement)
             VALUES ('{ACCOUNT}','rate@example.invalid','active','adult','{T0}','{T0}','minimal')"
        ),
    )
    .await;
    for (index, key) in ["a", "b", "c"].iter().enumerate() {
        item(
            &db,
            &format!("01956000-0000-4000-8000-00000000001{index}"),
            ACCOUNT,
            key,
            100 * (index as i64 + 1),
            None,
        )
        .await;
    }

    let page = metrics::ranked_with_weights(&db, &query(), Metric::LikesPerView, 10, 0, &QualityWeights::default())
        .await
        .expect("ranked");
    assert_eq!(page.views_rows, 0, "not one row carries a view count");
    assert_eq!(page.population, 3);
    // And the rows rank at zero rather than being dropped: the refusal belongs to
    // the caller, which has the metric name and can say what is missing.
    assert!(page.items.iter().all(|item| item.score == 0.0));
}
