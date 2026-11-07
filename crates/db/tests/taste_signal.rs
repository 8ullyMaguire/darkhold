//! M64-09 — the taste signal gravity pulls on, from the reader's own shelf.
//!
//! The failure this guards is a layer that is correct and inert: with `taste_signal = 0.0` everywhere, the mixture
//! `(1-g)*base + g*taste` is `base`, and a page can carry a published pull that never moved anything.
//!
//! Asserts:
//! 1. a candidate sharing half its entities with the shelf scores 0.5, not 1.0 and not "present";
//! 2. a candidate sharing none scores 0.0 and is **present** in the map — measured, and sharing nothing;
//! 3. a candidate carrying no entities at all is **absent** from the map — not measured, which is a different fact;
//! 4. an account with no shelf gets `None`, so a surface can say it has nothing to go on rather than report zeros.

use darkhold_db::DatabaseConfig;
use std::path::PathBuf;
use uuid::Uuid;

const ACCOUNT: &str = "01955000-0000-4000-8000-000000006001";
const PSEUD: &str = "01955000-0000-4000-8000-000000006002";
const SHELF: &str = "01955000-0000-4000-8000-000000006003";
const ITEM: &str = "01955000-0000-4000-8000-000000006004";
const SEED_WORK: &str = "01955000-0000-4000-8000-000000006005";
const HALF: &str = "01955000-0000-4000-8000-000000006006";
const NONE: &str = "01955000-0000-4000-8000-000000006007";
const UNTAGGED: &str = "01955000-0000-4000-8000-000000006008";
const TAG_A: &str = "01955000-0000-4000-8000-000000006009";
const TAG_B: &str = "01955000-0000-4000-8000-00000000600a";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("darkhold-tastesignal-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

async fn database(tag: &str) -> (darkhold_db::Database, PathBuf) {
    let dir = scratch_dir(tag);
    let db = match std::env::var("DARKHOLD_TEST_PG_URL") {
        Ok(admin_url) if !admin_url.is_empty() => {
            static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let name = format!(
                "lh_test_tastesig_{}_{}",
                std::process::id(),
                COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            );
            test_support::empty_postgres(&admin_url, &name)
                .await
                .expect("an empty PostgreSQL database")
        }
        _ => {
            let config =
                DatabaseConfig::new(format!("sqlite://{}/taste.sqlite?mode=rwc", dir.display()));
            darkhold_db::Database::connect(&config)
                .await
                .expect("open database")
        }
    };
    db.migrate().await.expect("migrate");
    (db, dir)
}

async fn exec(db: &darkhold_db::Database, statement: &str) {
    match db.backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(statement)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await
                .unwrap_or_else(|error| panic!("{statement}\n{error}"));
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(statement)
                .execute(db.postgres_pool().expect("postgres"))
                .await
                .unwrap_or_else(|error| panic!("{statement}\n{error}"));
        }
    }
}

#[tokio::test]
async fn the_signal_is_the_share_of_a_candidate_its_reader_already_knows() {
    let (db, dir) = database("share").await;

    exec(
        &db,
        &format!(
            "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version)
             VALUES ('{ACCOUNT}','active','shelf@example.invalid','adult',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;
    exec(
        &db,
        &format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at,
                                 updated_at, version)
             VALUES ('{PSEUD}','{ACCOUNT}','shelfowner','Shelf Owner','listed',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;
    for (id, title) in [
        (SEED_WORK, "Shelved"),
        (HALF, "Half"),
        (NONE, "Cold"),
        (UNTAGGED, "Untagged"),
    ] {
        exec(
            &db,
            &format!(
                "INSERT INTO works (id, owner_pseud_id, title, language, rating, visibility, lifecycle,
                                    created_at, updated_at, version)
                 VALUES ('{id}','{PSEUD}','{title}','en','general','public','published',
                         '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
            ),
        )
        .await;
    }
    for (id, canonical) in [(TAG_A, "slow burn"), (TAG_B, "found family")] {
        exec(
            &db,
            &format!(
                "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at)
                 VALUES ('{id}','tag','{canonical}','{canonical}','2026-01-01T00:00:00Z')"
            ),
        )
        .await;
    }

    // The shelf holds a work tagged `slow burn`; the reader's affinity set is therefore {TAG_A}.
    exec(
        &db,
        &format!(
            "INSERT INTO shelves (id, account_id, name, created_at, updated_at, version)
             VALUES ('{SHELF}','{ACCOUNT}','To read','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;
    exec(
        &db,
        &format!(
            "INSERT INTO library_items (id, account_id, work_id, source_key, source_work_key, title,
                                        source_url, created_at, updated_at, version)
             VALUES ('{ITEM}','{ACCOUNT}','{SEED_WORK}','manual','{SEED_WORK}','Shelved',
                     'https://example.invalid/shelved','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;
    exec(
        &db,
        &format!(
            "INSERT INTO shelf_items (id, shelf_id, library_item_id, created_at)
             VALUES ('01955000-0000-4000-8000-00000000600b','{SHELF}','{ITEM}','2026-01-01T00:00:00Z')"
        ),
    )
    .await;

    for (work, tag) in [
        (SEED_WORK, TAG_A),
        (HALF, TAG_A),
        (HALF, TAG_B),
        (NONE, TAG_B),
    ] {
        exec(
            &db,
            &format!(
                "INSERT INTO work_tags (work_id, node_id, prominence, added_at)
                 VALUES ('{work}','{tag}',1,'2026-01-01T00:00:00Z')"
            ),
        )
        .await;
    }

    let ids: Vec<Uuid> = [SEED_WORK, HALF, NONE, UNTAGGED]
        .iter()
        .map(|id| Uuid::parse_str(id).expect("uuid"))
        .collect();
    let signals = darkhold_db::taste_signal::for_account(&db, ACCOUNT, &ids)
        .await
        .expect("read signals")
        .expect("the account has a shelf");

    assert_eq!(
        signals.get(&Uuid::parse_str(HALF).expect("uuid")).copied(),
        Some(0.5),
        "one of the candidate's two entities is on the shelf: {signals:?}"
    );
    assert_eq!(
        signals.get(&Uuid::parse_str(NONE).expect("uuid")).copied(),
        Some(0.0),
        "measured, and sharing nothing — which is not the same as unmeasured: {signals:?}"
    );
    assert!(
        !signals.contains_key(&Uuid::parse_str(UNTAGGED).expect("uuid")),
        "a work with no entities cannot be scored, so it has no signal: {signals:?}"
    );

    // A different account with no shelf gets None rather than a page of zeros.
    let other = Uuid::parse_str(ACCOUNT).expect("uuid");
    assert!(
        darkhold_db::taste_signal::for_account(&db, &other.to_string(), &ids)
            .await
            .expect("read")
            .is_some(),
        "sanity: the account that owns the shelf does get a map"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn an_account_with_no_shelf_has_no_signal_rather_than_a_zero() {
    let (db, dir) = database("noshelf").await;
    exec(
        &db,
        &format!(
            "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version)
             VALUES ('{ACCOUNT}','active','bare@example.invalid','adult',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;
    let ids = vec![Uuid::parse_str(HALF).expect("uuid")];
    assert_eq!(
        darkhold_db::taste_signal::for_account(&db, ACCOUNT, &ids)
            .await
            .expect("read"),
        None,
        "no history is None, so a surface can say it has nothing to go on"
    );
    let _ = std::fs::remove_dir_all(dir);
}
