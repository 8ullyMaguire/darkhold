//! §12.7a rules 1 and 2 — the personalised engine may not rank by recency, and may not rank by tag
//! similarity either.
//!
//! `personalized_recommendations` used the taste profile as a **filter** and then ordered the
//! survivors by `updated_at DESC`. That is a tag filter wired to a recency sort: the published
//! study measured newest-first *below random* as a recommendation (0.0xxx f1@20, under chance's own
//! floor), and measured the pure tag-profile score as its worst result of all (0.0016 against
//! popularity's 0.0364). Neither may be the ranker, so the profile chooses the candidates and
//! aggregate kudos choose the order.
//!
//! The fixture is built so the two answers differ in a way no tie-break can hide: the *oldest* work
//! has the most kudos and the *newest* has one. Recency-first and popularity-first cannot both be
//! right about this list.
//!
//! Runs on whichever backend `DARKHOLD_TEST_PG_URL` names, and on SQLite when it is unset — the same
//! convention as `hit_rate.rs`, whose scaffolding is copied here because the db crate cannot depend
//! on `test_support` (that crate depends on this one).

use darkhold_db::Database;
use std::time::Duration;

const T0: i64 = 1_767_225_600; // 2026-01-01
const DAY: i64 = 86_400;

fn make_config(url: String) -> darkhold_db::DatabaseConfig {
    darkhold_db::DatabaseConfig {
        url,
        max_connections: 5,
        acquire_timeout: Duration::from_secs(10),
        slow_query_warn: Duration::ZERO,
    }
}

/// A scratch database on whichever backend is selected.
///
/// The PostgreSQL URL names an *admin* database and this creates a sibling per call: a shared
/// database accumulates applied migration checksums, so any in-place migration fix poisons every
/// later run against it.
async fn connect(tag: &str) -> Database {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "darkhold-order-{}-{}-{}",
        tag,
        uuid::Uuid::new_v4(),
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let url = match std::env::var("DARKHOLD_TEST_PG_URL") {
        Ok(admin) => {
            let name = format!(
                "lh_order_{}_{}",
                std::process::id(),
                uuid::Uuid::new_v4().simple()
            );
            let admin_db = Database::connect(&make_config(admin.clone()))
                .await
                .expect("connect to the admin database");
            sqlx::query(&format!("CREATE DATABASE {name}"))
                .execute(admin_db.postgres_pool().expect("postgres pool"))
                .await
                .expect("create a scratch database");
            admin_db.close().await;
            let (prefix, _) = admin
                .rsplit_once('/')
                .expect("the admin URL ends in a database");
            format!("{prefix}/{name}")
        }
        Err(_) => format!("sqlite://{}/darkhold.sqlite?mode=rwc", dir.display()),
    };
    let db = Database::connect(&make_config(url)).await.expect("connect");
    db.migrate().await.expect("migrate");
    db
}

fn rfc3339(unix: i64) -> String {
    let days = unix.div_euclid(DAY);
    let secs = unix.rem_euclid(DAY);
    let (h, mi, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    let (y, m, d) = (
        2026 + (days - 20_454) / 365,
        1 + (days - 20_454) % 365 / 30,
        1 + (days - 20_454) % 30,
    );
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// One statement, both dialects. `?N#u` names a uuid and `?N#t` a timestamp; SQLite drops the cast.
async fn insert(db: &Database, sqlite: &str, args: &[&str]) {
    match db.backend() {
        darkhold_db::Backend::Sqlite => {
            let sqlite = &sqlite
                .replace("#u", "")
                .replace("#t", "")
                .replace("::timestamptz", "");
            let mut q = sqlx::query(sqlite);
            for a in args {
                q = q.bind(*a);
            }
            q.execute(db.sqlite_pool().expect("sqlite pool"))
                .await
                .expect("insert");
        }
        darkhold_db::Backend::Postgres => {
            let postgres = (1..=6).fold(sqlite.to_string(), |acc, n| {
                acc.replace(&format!("?{n}#u"), &format!("${n}::uuid"))
                    .replace(&format!("?{n}#t"), &format!("${n}::timestamptz"))
                    .replace(&format!("?{n}"), &format!("${n}"))
            });
            let mut q = sqlx::query(&postgres);
            for a in args {
                match uuid::Uuid::parse_str(a) {
                    Ok(u) => q = q.bind(u),
                    // `views`/`kudos` are integer columns: bound as text, PostgreSQL rejects the
                    // insert with 42804 while SQLite accepts it silently, so the two backends
                    // disagree about a fixture that only ever ran on one of them.
                    Err(_) => match a.parse::<i64>() {
                        Ok(n) => q = q.bind(n),
                        Err(_) => q = q.bind(*a),
                    },
                };
            }
            q.execute(db.postgres_pool().expect("postgres pool"))
                .await
                .expect("insert");
        }
    }
}

struct Fixture {
    db: Database,
    account: String,
    /// (work id, kudos) in the order they were created — creation order, not answer order.
    works: Vec<(String, i64)>,
}

impl Fixture {
    /// A reader with a taste profile on `node`, and three works carrying it: oldest-most-kudos,
    /// middle, newest-least-kudos.
    async fn build(tag: &str) -> Self {
        let db = connect(tag).await;
        let run = uuid::Uuid::new_v4().to_string();
        let account = uuid::Uuid::new_v4().to_string();
        let pseud = uuid::Uuid::new_v4().to_string();
        insert(
            &db,
            "INSERT INTO accounts (id, email, created_at, updated_at) \
             VALUES (?1#u, ?2, '2026-01-01T00:00:00Z'::timestamptz, '2026-01-01T00:00:00Z')",
            &[&account, &format!("order-{tag}-{run}@example.com")],
        )
        .await;
        insert(
            &db,
            "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at) \
             VALUES (?1#u, ?2#u, ?3, ?3, '2026-01-01T00:00:00Z'::timestamptz, '2026-01-01T00:00:00Z')",
            &[&pseud, &account, &format!("o{tag}-{run}")],
        )
        .await;

        // The works belong to a DIFFERENT pseud. The query excludes the reader's own works by
        // design (`owner_pseud_id NOT IN (SELECT id FROM pseuds WHERE account_id = ?)`), so a
        // fixture that publishes as the reader gets an empty list and would look like a broken
        // join.
        let author_account = uuid::Uuid::new_v4().to_string();
        let author_pseud = uuid::Uuid::new_v4().to_string();
        insert(
            &db,
            "INSERT INTO accounts (id, email, created_at, updated_at) \
             VALUES (?1#u, ?2, '2026-01-01T00:00:00Z'::timestamptz, '2026-01-01T00:00:00Z')",
            &[&author_account, &format!("author-{tag}-{run}@example.com")],
        )
        .await;
        insert(
            &db,
            "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at) \
             VALUES (?1#u, ?2#u, ?3, ?3, '2026-01-01T00:00:00Z'::timestamptz, '2026-01-01T00:00:00Z')",
            &[&author_pseud, &author_account, &format!("a{tag}-{run}")],
        )
        .await;

        // The profile's signals are a JSON array of node ids: `json_each`/`json_array_elements_text`
        // read them in the query, so a bare string here would match nothing and score everyone
        // equally — a fixture bug that looks like a ranking bug.
        let node = format!("\"{tag}\"");
        insert(
            &db,
            "INSERT INTO taste_profiles (account, signals, computed_at) \
             VALUES (?1, ?2, '2026-01-01T00:00:00Z')",
            &[&account, &format!("[{node}]")],
        )
        .await;
        insert(
            &db,
            "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at) \
             VALUES (?1, 'tag', ?1, ?1, '2026-01-01T00:00:00Z'::timestamptz)",
            &[tag],
        )
        .await;

        // (title, days after T0, kudos) — deliberately anti-correlated: the older it is, the more
        // popular.
        let plan = [
            ("OldestPopular", 0i64, 50i64),
            ("Middle", 60, 10),
            ("NewestQuiet", 200, 1),
        ];
        let mut works = Vec::new();
        for (title, day, kudos) in plan {
            let w = uuid::Uuid::new_v4().to_string();
            let at = rfc3339(T0 + day * DAY);
            insert(
                &db,
                "INSERT INTO works (id, owner_pseud_id, title, lifecycle, visibility, created_at, \
                 updated_at, generated_content_posture) \
                 VALUES (?1#u, ?2#u, ?3, 'published', 'public', ?4#t, ?4#t, 'forbid')",
                &[&w, &author_pseud, title, &at],
            )
            .await;
            insert(
                &db,
                // `weight` was renamed to `prominence` by 0126, and the categorical column is now
                // `role` with its own default — naming `weight` here is a column that no longer
                // exists, which the SQLite error reports as "no column named weight".
                "INSERT INTO work_tags (work_id, node_id, prominence, added_at) \
                 VALUES (?1#u, ?2, 0, ?3#t)",
                &[&w, tag, &at],
            )
            .await;
            insert(
                &db,
                "INSERT INTO work_metric_aggregates (work_id, views, complete_reads, reactions, \
                 kudos, bookmarks, collection_adds, reviews, updated_at) \
                 VALUES (?1#u, ?2, 0, 0, ?2, 0, 0, 0, ?3#t)",
                &[&w, &kudos.to_string(), &at],
            )
            .await;
            works.push((w, kudos));
        }
        Self { db, account, works }
    }

    async fn order(&self) -> Vec<String> {
        darkhold_db::discovery::personalized_recommendations(&self.db, &self.account, None, 20)
            .await
            .expect("personalized")
            .iter()
            .map(|w| w.to_string())
            .collect()
    }
}

/// The assertion the defect fails: popularity first, newest last.
#[tokio::test]
async fn the_personalised_engine_orders_by_popularity_and_not_by_recency() {
    let fixture = Fixture::build("naruto").await;
    let served = fixture.order().await;
    let expected: Vec<String> = fixture.works.iter().map(|(w, _)| w.clone()).collect();

    assert_eq!(
        served, expected,
        "kudos 50/10/1 with the newest work last: the order must be popularity, got {served:?} \
         against an expected {expected:?}"
    );

    // The premise, stated as an assertion so a fixture that stops being anti-correlated fails here
    // rather than passing for the wrong reason.
    let newest = &fixture.works[2].0;
    let oldest = &fixture.works[0].0;
    assert_ne!(
        served.first(),
        Some(newest),
        "PREMISE: recency-first would have put {newest} first; if it did, this test proves nothing"
    );
    assert_eq!(
        served.first(),
        Some(oldest),
        "PREMISE: the most-kudos work is the oldest one, and it must lead"
    );
}

/// With no kudos at all, popularity cannot separate the works, so recency is the tie-break — and it
/// must still be deterministic, because a tie broken by row order differs between the two backends.
#[tokio::test]
async fn with_no_kudos_the_tie_breaks_on_recency_and_stays_deterministic() {
    let fixture = Fixture::build("bleach").await;
    for (work, _) in &fixture.works {
        insert(
            &fixture.db,
            "UPDATE work_metric_aggregates SET kudos = 0, views = 0 WHERE work_id = ?1#u",
            &[work],
        )
        .await;
    }
    let first = fixture.order().await;
    let second = fixture.order().await;
    assert_eq!(
        first, second,
        "two calls on the same state must agree (§47.9)"
    );
    assert_eq!(
        first.len(),
        3,
        "the tag profile still generates all three candidates, got {first:?}"
    );
    let newest = &fixture.works[2].0;
    assert_eq!(
        first.first(),
        Some(newest),
        "an all-zero popularity gives recency the tie-break, so the newest leads: {first:?}"
    );
}

/// A profile whose signals match nothing generates nothing — the drop is the engine's answer, not a
/// list of arbitrary works padded out to `limit`.
#[tokio::test]
async fn a_profile_that_matches_no_work_returns_nothing_rather_than_padding() {
    let fixture = Fixture::build("hobbit").await;
    // The account is a uuid literal inlined rather than bound: `taste_profiles.account` is TEXT,
    // and a uuid bind against it is `text = uuid` on PostgreSQL (42883) while SQLite accepts it.
    let statement = format!(
        "UPDATE taste_profiles SET signals = '[\"a-tag-no-work-carries\"]' WHERE account = '{}'",
        fixture.account
    );
    match fixture.db.backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(&statement)
                .execute(fixture.db.sqlite_pool().expect("sqlite"))
                .await
                .expect("retarget the profile");
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(&statement)
                .execute(fixture.db.postgres_pool().expect("postgres"))
                .await
                .expect("retarget the profile");
        }
    };
    let served = fixture.order().await;
    assert!(
        served.is_empty(),
        "no candidate carries the profile's signal, so the honest answer is an empty list: {served:?}"
    );
}
