//! M64-07 — crosspost union: one story's engagement summed across every archive, ranked once.
//!
//! The rule is M63-04's: a story is identified by normalised title + author. The test makes the two failure modes visible —
//! ranking copies separately (the same story three times with a third of its evidence) and merging distinct stories that happen to
//! share a title — and pins the representative to a **readable** copy, so an archived private draft cannot become the face of a
//! popular story.
//!
//! Asserts:
//! 1. three copies on three archives are one group with the summed count and `sources: 3`;
//! 2. the sum is order-independent and the title/author spelling differences do not split the group;
//! 3. two different authors writing the same title are two groups;
//! 4. an unpublished copy is not counted and cannot be the representative;
//! 5. a story with no readable copy is not ranked at all.

use darkhold_db::crosspost::union_ranking;
use darkhold_db::{Backend, Database, DatabaseConfig};
use std::path::PathBuf;

const AUTHOR: &str = "01955c00-0000-4000-8000-000000000001";
const PSEUD: &str = "01955c00-0000-4000-8000-000000000002";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-crosspost-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

async fn database(tag: &str) -> (Database, PathBuf) {
    let dir = scratch_dir(tag);
    let db = match std::env::var("DARKHOLD_TEST_PG_URL") {
        Ok(admin_url) if !admin_url.is_empty() => {
            static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let name = format!(
                "lh_test_crosspost_{}_{}",
                std::process::id(),
                COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            );
            test_support::empty_postgres(&admin_url, &name)
                .await
                .expect("an empty PostgreSQL database")
        }
        _ => {
            let config = DatabaseConfig::new(format!(
                "sqlite://{}/crosspost.sqlite?mode=rwc",
                dir.display()
            ));
            Database::connect(&config).await.expect("connect")
        }
    };
    db.migrate().await.expect("migrate");
    (db, dir)
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

/// Insert a work plus one imported copy of it from one archive.
async fn copy(
    db: &Database,
    index: i32,
    title: &str,
    author: &str,
    source: &str,
    likes: i64,
    // `(lifecycle, visibility)` as one value: the pair is what "readable" means, and as two parameters it pushes this helper past
    // clippy's argument limit for a fixture that only ever uses two combinations.
    state: (&str, &str),
) -> String {
    let (lifecycle, visibility) = state;
    let work = format!("01955d00-0000-4000-8000-0000000000{index:02}");
    exec(
        db,
        &format!(
            "INSERT INTO works (id, owner_pseud_id, title, visibility, lifecycle, created_at,
                                updated_at, version)
             VALUES ('{work}','{PSEUD}','{title}','{visibility}','{lifecycle}',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO library_items (id, account_id, work_id, source_key, source_work_key, title,
                                        author_text, summary, status, source_url, likes,
                                        provenance_json, created_at, updated_at, version)
             VALUES ('01955e00-0000-4000-8000-0000000000{index:02}','{AUTHOR}','{work}','{source}',
                     'k{index}','{title}','{author}','','complete','https://example.invalid/{index}',
                     {likes},'{{}}','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;
    work
}

#[tokio::test]
async fn one_story_ranks_once_with_all_of_its_archives() {
    let (db, dir) = database("union").await;
    exec(
        &db,
        &format!(
            "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version)
             VALUES ('{AUTHOR}','active','x@example.invalid','adult',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;
    exec(
        &db,
        &format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at,
                                 updated_at, version)
             VALUES ('{PSEUD}','{AUTHOR}','poster','Poster','listed',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;

    // 1 + 2. The same story on three archives, with spacing and case differences the normalisation must absorb.
    copy(
        &db,
        1,
        "The Long Road",
        "Someone",
        "ao3",
        40,
        ("published", "public"),
    )
    .await;
    copy(
        &db,
        2,
        "the long road ",
        " someone ",
        "sb",
        25,
        ("published", "public"),
    )
    .await;
    copy(
        &db,
        3,
        "THE LONG ROAD",
        "SOMEONE",
        "ffnet",
        7,
        ("published", "public"),
    )
    .await;

    // 3. A different author with the same title is a different story.
    copy(
        &db,
        4,
        "The Long Road",
        "Else",
        "ao3",
        5,
        ("published", "public"),
    )
    .await;

    // 4. An unpublished copy of a second story: its likes must not count, and it cannot be the face.
    copy(
        &db,
        5,
        "Hidden Draft",
        "Someone",
        "sv",
        500,
        ("draft", "private"),
    )
    .await;

    // 5. A story with no readable copy at all.
    copy(
        &db,
        6,
        "Nowhere",
        "Someone",
        "qq",
        300,
        ("draft", "private"),
    )
    .await;

    let ranked = union_ranking(&db, 50).await.expect("ranking");
    assert_eq!(
        ranked.len(),
        2,
        "one story for the union, one for the same title by another author: {ranked:?}"
    );

    let long_road = &ranked[0];
    assert_eq!(
        long_road.total_likes, 72,
        "40 + 25 + 7, across three archives"
    );
    assert_eq!(long_road.sources, 3);
    assert!(
        long_road.work_id == "01955d00-0000-4000-8000-000000000001",
        "the representative is the lowest-id readable copy: {long_road:?}"
    );

    let other = &ranked[1];
    assert_eq!(
        other.total_likes, 5,
        "another author's work of the same name"
    );
    assert_eq!(other.sources, 1);

    // 4 + 5, said explicitly: the 500-like draft and the 300-like unreadable story are absent, not ranked low.
    assert!(
        ranked.iter().all(|group| group.total_likes < 500),
        "an unpublished copy contributes nothing: {ranked:?}"
    );

    // The limit bounds the answer, not the sum.
    let one = union_ranking(&db, 1).await.expect("ranking");
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].total_likes, 72);

    let _ = std::fs::remove_dir_all(dir);
}
