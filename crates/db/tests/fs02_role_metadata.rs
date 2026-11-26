//! FS-02 — reviewed role metadata, on both engines (SQLite by default, PostgreSQL with
//! `DARKHOLD_TEST_PG_URL`).
//!
//! The migration is the artefact under test: after a fixture supplies the real works and cast,
//! this test re-applies the actual 0221 SQL file with `sqlx::raw_sql`, then reads the result through
//! the same store the API uses. A mutant in either arm of the migration must fail here.

use std::time::Duration;

use darkhold_db::{work_characters, Backend, Database};

const NOW: &str = "2026-11-26T00:00:00Z";
const POWER_PLAYS: &str = "3569bf92-d6e3-4441-9163-75328e3bc349";
const BUCKET_LIST: &str = "84b7455c-9166-4415-9aee-3f105b7c73b7";
const ATTENDANTS: &str = "8f841724-f791-437f-8c38-d89cd336886a";
const HARRY: &str = "77d9c8a4-356d-4b81-b503-61a1456a1baf";
const TONKS: &str = "69a9f0bf-abba-49d9-bc4c-86cad9f6a627";
const HERMIONE: &str = "fa52bf68-6218-4e3d-b5d3-bb9663aa9edb";
const SUSAN: &str = "2d2c5693-2642-4b14-ad47-3c65fb153185";
const JAMES: &str = "7c955608-4e63-48c9-8f84-b4222aad5d1e";
const LILY: &str = "4b6d4099-875d-47f9-8d52-b1f36294b22d";
const DAPHNE: &str = "932e99fd-59b5-4f6f-b3f8-857f5418afa7";
const TRACEY: &str = "60bc3811-0cd4-44d9-b837-a0cfac3f6320";
const NARRATOR: &str = "5f3819e5-8391-50c0-a3eb-28da9e40d9c9";
const MENTOR: &str = "63c312ef-7c11-5f63-83d7-6d1b9de2c500";
const LOVE_INTEREST: &str = "bcf4319f-1c02-58cc-bc9f-d837d04e0462";
const FRIEND: &str = "030cd99e-e46d-5c93-b00e-5ba4ca96c544";
const PARENT: &str = "262ac5ce-9f64-56f3-a607-06a276437e38";
const ATTENDANT: &str = "19386f25-6856-5377-ac21-334456845ca1";

fn config(url: String) -> darkhold_db::DatabaseConfig {
    darkhold_db::DatabaseConfig {
        url,
        max_connections: 2,
        acquire_timeout: Duration::from_secs(60),
        slow_query_warn: Duration::ZERO,
    }
}

async fn connect(tag: &str) -> Database {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "darkhold-fs02-{}-{}-{}",
        tag,
        uuid::Uuid::new_v4().simple(),
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let url = match std::env::var("DARKHOLD_TEST_PG_URL") {
        Ok(admin) => {
            let name = format!("lh_fs02_{}_{}", std::process::id(), uuid::Uuid::new_v4().simple());
            let admin_db = Database::connect(&config(admin.clone()))
                .await
                .expect("connect admin");
            sqlx::query(&format!("CREATE DATABASE {name}"))
                .execute(admin_db.postgres_pool().expect("pg"))
                .await
                .expect("create scratch db");
            admin_db.close().await;
            let (prefix, _) = admin.rsplit_once('/').expect("admin url has a database");
            format!("{prefix}/{name}")
        }
        Err(_) => format!("sqlite://{}/darkhold.sqlite?mode=rwc", dir.display()),
    };
    let db = Database::connect(&config(url)).await.expect("connect");
    db.migrate().await.expect("migrate");
    db
}

async fn exec(db: &Database, sqlite: &str, postgres: &str) {
    let sql = db.sql(sqlite, postgres);
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("fixture (sqlite)");
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .execute(db.postgres_pool().expect("pg"))
                .await
                .expect("fixture (postgres)");
        }
    };
}

async fn count(db: &Database, sqlite: &str, postgres: &str) -> i64 {
    let sql = db.sql(sqlite, postgres);
    match db.backend() {
        Backend::Sqlite => sqlx::query_scalar(&sql)
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("count (sqlite)"),
        Backend::Postgres => sqlx::query_scalar(&sql)
            .fetch_one(db.postgres_pool().expect("pg"))
            .await
            .expect("count (postgres)"),
    }
}

async fn fixture(db: &Database) -> String {
    let account = uuid::Uuid::new_v4().to_string();
    let pseud = uuid::Uuid::new_v4().to_string();
    exec(
        db,
        &format!(
            "INSERT INTO accounts (id, email, status, age_state, created_at, updated_at, permission_statement)
             VALUES ('{account}', '{account}@example.invalid', 'active', 'adult', '{NOW}', '{NOW}', 'minimal')"
        ),
        &format!(
            "INSERT INTO accounts (id, email, status, age_state, created_at, updated_at, permission_statement)
             VALUES ('{account}', '{account}@example.invalid', 'active', 'adult', '{NOW}', '{NOW}', 'minimal')"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at)
             VALUES ('{pseud}', '{account}', 'fs02-author', 'FS02', '{NOW}', '{NOW}')"
        ),
        &format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at)
             VALUES ('{pseud}', '{account}', 'fs02-author', 'FS02', '{NOW}', '{NOW}')"
        ),
    )
    .await;

    for (id, title) in [
        (POWER_PLAYS, "Power Plays"),
        (BUCKET_LIST, "Harry''s Bucket List"),
        (ATTENDANTS, "The Attendants Chapters"),
    ] {
        exec(
            db,
            &format!(
                "INSERT INTO works (id, owner_pseud_id, title, lifecycle, visibility, created_at, updated_at)
                 VALUES ('{id}', '{pseud}', '{title}', 'published', 'public', '{NOW}', '{NOW}')"
            ),
            &format!(
                "INSERT INTO works (id, owner_pseud_id, title, lifecycle, visibility, created_at, updated_at)
                 VALUES ('{id}', '{pseud}', '{title}', 'published', 'public', '{NOW}', '{NOW}')"
            ),
        )
        .await;
    }

    for (id, norm, canonical) in [
        (HARRY, "harrypotter", "Harry Potter"),
        (TONKS, "nymphadoratonks", "Nymphadora Tonks"),
        (HERMIONE, "hermionegranger", "Hermione Granger"),
        (SUSAN, "susanbones", "Susan Bones"),
        (JAMES, "jamespotter", "James Potter"),
        (LILY, "lilypotter", "Lily Potter"),
        (DAPHNE, "daphnegreengrass", "Daphne Greengrass"),
        (TRACEY, "traceydavis", "Tracey Davis"),
    ] {
        exec(
            db,
            &format!(
                "INSERT OR IGNORE INTO canonical_entities (id, kind, canonical, norm, created_at)
                 VALUES ('{id}', 'character', '{canonical}', '{norm}', '{NOW}')"
            ),
            &format!(
                "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at)
                 VALUES ('{id}', 'character', '{canonical}', '{norm}', '{NOW}')
                 ON CONFLICT (kind, norm) DO NOTHING"
            ),
        )
        .await;
    }

    let initial = [
        (POWER_PLAYS, HARRY),
        (POWER_PLAYS, TONKS),
        (POWER_PLAYS, HERMIONE),
        (POWER_PLAYS, SUSAN),
        (BUCKET_LIST, HERMIONE),
        (ATTENDANTS, HARRY),
        (ATTENDANTS, HERMIONE),
        (ATTENDANTS, DAPHNE),
        (ATTENDANTS, TRACEY),
    ];
    for (work, character) in initial {
        exec(
            db,
            &format!(
                "INSERT OR IGNORE INTO work_characters (work_id, character_node_id, prominence, is_pov, added_at)
                 VALUES ('{work}', '{character}', 'supporting', 0, '{NOW}')"
            ),
            &format!(
                "INSERT INTO work_characters (work_id, character_node_id, prominence, is_pov, added_at)
                 VALUES ('{work}'::uuid, '{character}', 'supporting', 0, '{NOW}')
                 ON CONFLICT DO NOTHING"
            ),
        )
        .await;
    }
    pseud
}

async fn replay_migration(db: &Database) {
    let name = match db.backend() {
        Backend::Sqlite => "sqlite/0221_fs02_role_metadata.sql",
        Backend::Postgres => "postgres/0221_fs02_role_metadata.sql",
    };
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../migrations")
        .join(name);
    let sql = std::fs::read_to_string(path).expect("read 0221 migration");
    match db.backend() {
        Backend::Sqlite => {
            sqlx::raw_sql(&sql)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("replay 0221 (sqlite)");
        }
        Backend::Postgres => {
            sqlx::raw_sql(&sql)
                .execute(db.postgres_pool().expect("pg"))
                .await
                .expect("replay 0221 (postgres)");
        }
    };
}

fn character<'a>(rows: &'a [work_characters::WorkCharacter], id: &str) -> &'a work_characters::WorkCharacter {
    rows.iter().find(|c| c.character_node_id == id).unwrap_or_else(|| panic!("missing character {id}"))
}

#[tokio::test]
async fn reviewed_roles_are_stored_and_unobserved_roles_stay_absent() {
    let db = connect("roles").await;
    fixture(&db).await;
    replay_migration(&db).await;

    assert_eq!(
        count(
            &db,
            "SELECT count(*) FROM canonical_entities WHERE kind='role'",
            "SELECT count(*) FROM canonical_entities WHERE kind='role'"
        )
        .await,
        6
    );
    assert_eq!(
        count(
            &db,
            "SELECT count(*) FROM work_character_roles",
            "SELECT count(*) FROM work_character_roles"
        )
        .await,
        11
    );

    let power = work_characters::characters_for_work(&db, POWER_PLAYS)
        .await
        .expect("characters for Power Plays");
    let harry = character(&power, HARRY);
    assert_eq!(harry.prominence, "protagonist");
    assert!(harry.is_pov);
    assert!(harry.roles.iter().any(|r| r == NARRATOR));
    let tonks = character(&power, TONKS);
    assert_eq!(tonks.prominence, "supporting");
    assert!(tonks.roles.iter().any(|r| r == MENTOR));
    assert!(tonks.roles.iter().any(|r| r == LOVE_INTEREST));
    assert!(character(&power, SUSAN).roles.is_empty());

    let bucket = work_characters::characters_for_work(&db, BUCKET_LIST)
        .await
        .expect("characters for Harry's Bucket List");
    let added_harry = character(&bucket, HARRY);
    assert_eq!(added_harry.prominence, "protagonist");
    assert!(added_harry.is_pov);
    assert!(added_harry.roles.iter().any(|r| r == NARRATOR));
    assert!(character(&bucket, HERMIONE).roles.iter().any(|r| r == FRIEND));

    let attendants = work_characters::characters_for_work(&db, ATTENDANTS)
        .await
        .expect("characters for The Attendants Chapters");
    assert!(character(&attendants, JAMES).roles.iter().any(|r| r == PARENT));
    assert!(character(&attendants, LILY).roles.iter().any(|r| r == PARENT));
    assert!(character(&attendants, DAPHNE).roles.iter().any(|r| r == ATTENDANT));
    assert!(character(&attendants, TRACEY).roles.iter().any(|r| r == ATTENDANT));
}
