//! M64-08's query half: works reached through a declared fandom adjacency.
//!
//! The map is configuration and is tested in `darkhold-domain`; what this file pins is what happens once rows are involved:
//!
//! 1. a work in a neighbouring fandom is found and carries the operator's **weight**, not a popularity score pretending to be one;
//! 2. the seed is excluded, so "related to Ward" never means Ward;
//! 3. an unpublished or private work in the neighbouring fandom is not offered — an adjacency is a reason to look, not a permission;
//! 4. a fandom the map does not mention yields nothing, and so does an empty map. Most fandoms are adjacent to nothing, and a surface
//!    asking about one must be able to tell "no relation declared" from "the query broke";
//! 5. the weight decides the order before likes do: a `0.9` edge's quiet work outranks a `0.2` edge's popular one.

use darkhold_db::fandom_graph::related_works;
use darkhold_db::{Backend, Database, DatabaseConfig};

const T0: &str = "2026-01-01T00:00:00Z";
const ACCOUNT: &str = "01955c00-0000-4000-8000-00000000f000";
const PSEUD: &str = "01955c00-0000-4000-8000-00000000f001";
const WORM: &str = "01955c00-0000-4000-8000-00000000f010";
const WARD: &str = "01955c00-0000-4000-8000-00000000f011";
const NARUTO: &str = "01955c00-0000-4000-8000-00000000f012";
const SEED: &str = "01955c00-0000-4000-8000-00000000f020";
const WARD_WORK: &str = "01955c00-0000-4000-8000-00000000f021";
const NARUTO_WORK: &str = "01955c00-0000-4000-8000-00000000f022";
const PRIVATE_WARD: &str = "01955c00-0000-4000-8000-00000000f023";
const DRAFT_WARD: &str = "01955c00-0000-4000-8000-00000000f024";

fn scratch_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-fg-{tag}-{}-{:?}",
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
                "lh_test_fg_{}_{}",
                std::process::id(),
                COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            );
            test_support::empty_postgres(&admin_url, &name)
                .await
                .expect("an empty PostgreSQL database")
        }
        _ => {
            let config =
                DatabaseConfig::new(format!("sqlite://{}/fg.sqlite?mode=rwc", dir.display()));
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

async fn seed_account(db: &Database) {
    exec(
        db,
        &format!(
            "INSERT INTO accounts (id, email, status, age_state, created_at, updated_at, permission_statement)
             VALUES ('{ACCOUNT}','fg@example.invalid','active','adult','{T0}','{T0}','minimal')"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at)
             VALUES ('{PSEUD}','{ACCOUNT}','fgwriter','fgwriter','{T0}','{T0}')"
        ),
    )
    .await;
}

async fn fandom(db: &Database, id: &str, name: &str) {
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

async fn work(db: &Database, id: &str, title: &str, lifecycle: &str, visibility: &str, kudos: i64) {
    exec(
        db,
        &format!(
            "INSERT INTO works (id, owner_pseud_id, title, summary, lifecycle, visibility, completion,
                                created_at, updated_at, generated_content_posture)
             VALUES ('{id}','{PSEUD}','{title}','','{lifecycle}','{visibility}','complete','{T0}','{T0}','forbid')"
        ),
    )
    .await;
    if kudos > 0 {
        exec(
            db,
            &format!(
                "INSERT INTO work_metric_aggregates (work_id, views, complete_reads, reactions, kudos,
                                                    bookmarks, collection_adds, reviews, updated_at)
                 VALUES ('{id}',0,0,0,{kudos},0,0,0,'{T0}')"
            ),
        )
        .await;
    }
}

async fn tag(db: &Database, work_id: &str, node_id: &str) {
    exec(
        db,
        &format!(
            "INSERT INTO work_tags (work_id, node_id, prominence, added_at)
             VALUES ('{work_id}','{node_id}',1,'{T0}')"
        ),
    )
    .await;
}

async fn seed(db: &Database) {
    seed_account(db).await;
    fandom(db, WORM, "Worm").await;
    fandom(db, WARD, "Ward").await;
    fandom(db, NARUTO, "Naruto").await;

    work(db, SEED, "The seed", "published", "public", 500).await;
    tag(db, SEED, WORM).await;
    // A quiet work in the strong neighbour, a popular one in the weak neighbour.
    work(db, WARD_WORK, "Ward story", "published", "public", 3).await;
    tag(db, WARD_WORK, WARD).await;
    work(db, NARUTO_WORK, "Naruto story", "published", "public", 900).await;
    tag(db, NARUTO_WORK, NARUTO).await;
    // Same neighbour, but not readable.
    work(db, PRIVATE_WARD, "Ward private", "published", "private", 50).await;
    tag(db, PRIVATE_WARD, WARD).await;
    work(db, DRAFT_WARD, "Ward draft", "draft", "public", 50).await;
    tag(db, DRAFT_WARD, WARD).await;
}

/// The lime green case: *Worm* and *Ward* are one serial in two halves, and the operator says so with a weight.
#[tokio::test]
async fn a_declared_neighbour_is_reached_with_its_weight_and_nothing_unreadable() {
    let db = connect("related").await;
    seed(&db).await;

    let fandoms = vec![
        ("Ward".to_owned(), 0.9),
        ("Naruto".to_owned(), 0.2),
    ];
    let found = related_works(&db, &fandoms, &[SEED.to_owned()], 10)
        .await
        .expect("related works");

    let titles: Vec<&str> = found.iter().map(|w| w.title.as_str()).collect();
    assert_eq!(
        titles,
        vec!["Ward story", "Naruto story"],
        "only readable works, and the weight orders them: {titles:?}"
    );
    assert_eq!(found[0].weight, 0.9);
    assert_eq!(found[0].fandom, "Ward", "the operator's spelling, not the stored norm");
    assert_eq!(found[0].likes, 3, "the endorsement count travels for the caller");
    assert_eq!(found[1].weight, 0.2);

    // The seed is never its own relation, even though it is in a neighbouring fandom's map.
    assert!(!found.iter().any(|w| w.work_id == SEED));

    // A fandom the map does not mention knows nothing — and says so with an empty page, not an error.
    let unmapped = related_works(&db, &[("Some Fandom Nobody Mapped".to_owned(), 0.5)], &[], 10)
        .await
        .expect("unmapped");
    assert!(unmapped.is_empty());

    // An empty map is the shipped state of every instance until an operator publishes one.
    let nothing = related_works(&db, &[], &[], 10).await.expect("empty map");
    assert!(nothing.is_empty());
}

/// The weight is the claim; likes only break a tie inside one edge.
#[tokio::test]
async fn the_edge_weight_decides_before_the_endorsement_count() {
    let db = connect("weights").await;
    seed(&db).await;

    let found = related_works(
        &db,
        &[("Naruto".to_owned(), 0.2), ("Ward".to_owned(), 0.9)],
        &[],
        10,
    )
    .await
    .expect("related works");
    assert_eq!(
        found[0].title, "Ward story",
        "3 kudos behind a 0.9 edge outrank 900 behind a 0.2: {found:?}"
    );
}
