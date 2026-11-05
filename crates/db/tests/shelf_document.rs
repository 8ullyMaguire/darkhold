//! M45-72 — the shelf page reads the view rows and their last run, pinned first.

use std::time::Duration;

use darkhold_db::{Backend, Database};

async fn connect(tag: &str) -> Database {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "darkhold-explain-{}-{}-{}",
        tag,
        uuid::Uuid::new_v4(),
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let url = match std::env::var("DARKHOLD_TEST_PG_URL") {
        Ok(admin) => {
            let name = format!(
                "lh_explain_{}_{}",
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
                .expect("admin URL ends in a database");
            format!("{prefix}/{name}")
        }
        Err(_) => format!("sqlite://{}/darkhold.sqlite?mode=rwc", dir.display()),
    };
    let db = Database::connect(&make_config(url)).await.expect("connect");
    db.migrate().await.expect("migrate");
    db
}

/// Everything a reader does that carries tags, in one place.
async fn exec(db: &Database, sqlite: &str, postgres: &str, binds: &[&str]) {
    match db.backend() {
        Backend::Sqlite => {
            let mut q = sqlx::query(sqlite);
            for value in binds {
                q = q.bind(*value);
            }
            q.execute(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("insert");
        }
        Backend::Postgres => {
            let mut q = sqlx::query(postgres);
            for value in binds {
                q = q.bind(*value);
            }
            q.execute(db.postgres_pool().expect("pg"))
                .await
                .expect("insert");
        }
    }
}

fn make_config(url: String) -> darkhold_db::DatabaseConfig {
    darkhold_db::DatabaseConfig {
        url,
        max_connections: 2,
        acquire_timeout: Duration::from_secs(5),
        slow_query_warn: Duration::ZERO,
    }
}

const T0: &str = "2026-11-01T00:00:00Z";

#[tokio::test]
async fn shelves_are_pinned_first_and_a_view_without_a_run_says_so() {
    let db = connect("shelfdocs").await;
    let account = uuid::Uuid::new_v4().to_string();
    exec(&db,
        "INSERT INTO accounts (id, email, status, age_state, created_at, updated_at) VALUES (?, ?, 'active', 'adult', ?, ?)",
        "INSERT INTO accounts (id, email, status, age_state, created_at, updated_at) VALUES ($1::uuid, $2, 'active', 'adult', $3, $4)",
        &[&account, &format!("{}@example.invalid", account), T0, T0]).await;

    // Real UUIDs, because `saved_views.id` is a UUID on PostgreSQL and `'v1'` is not one. One spelling
    // for both engines: a test that needs a different id per engine is testing its own scaffolding.
    const V1: &str = "11111111-1111-1111-1111-111111111111";
    const V2: &str = "22222222-2222-2222-2222-222222222222";

    // Two views: "alpha" was run once, "zeta" never. Zeta is pinned, so it opens the page.
    let catalogue_doc = r#"{"version":1,"kind":"catalogue","query":null,"params":"tag:dark AND trope:system-grind"}"#;
    let library_doc = r#"{"version":1,"kind":"library","query":{"shelves":["grimdark"],"tags":[],"statuses":[],"source":null,"updated_since":null,"sort":"recent"},"params":null}"#;
    for (id, name, doc, pinned) in [
        (V1, "alpha", library_doc, 0),
        (V2, "zeta", catalogue_doc, 1),
    ] {
        let sqlite = format!(
            "INSERT INTO saved_views (id, account_id, name, query_json, query_version, sort, scope, pinned, is_public, created_at, updated_at, version)
             VALUES ('{id}', ?, '{name}', ?, 1, 'recent', 'public', {pinned}, 0, '{T0}', '{T0}', 1)"
        );
        let postgres = format!(
            "INSERT INTO saved_views (id, account_id, name, query_json, query_version, sort, scope, pinned, is_public, created_at, updated_at, version)
             VALUES ('{id}'::uuid, $1::uuid, '{name}', $2, 1, 'recent', 'public', {}, FALSE, '{T0}', '{T0}', 1)",
            if pinned == 1 { "TRUE" } else { "FALSE" }
        );
        exec(&db, &sqlite, &postgres, &[&account, doc]).await;
    }
    exec(
        &db,
        "INSERT INTO saved_query_runs (saved_view_id, ran_at, seen_ids_json, seen_count)
         VALUES ('11111111-1111-1111-1111-111111111111', ?, '[]', 12)",
        "INSERT INTO saved_query_runs (saved_view_id, ran_at, seen_ids_json, seen_count)
         VALUES ('11111111-1111-1111-1111-111111111111'::uuid, $1, '[]', 12)",
        &[T0],
    )
    .await;

    let rows = darkhold_db::shelf_document::shelves_for_account(&db, &account)
        .await
        .expect("rows");
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].name,
        "zeta",
        "pinned opens the page: {:?}",
        rows.iter().map(|r| &r.name).collect::<Vec<_>>()
    );
    assert!(rows[0].pinned);
    assert!(rows[0].last_run.is_none(), "zeta never ran");
    assert_eq!(rows[1].name, "alpha");
    assert_eq!(rows[1].last_run.as_ref().map(|(_, seen)| *seen), Some(12));

    // The documents round-trip through the domain renderer, so the page shows a query and not an empty one.
    assert_eq!(
        darkhold_domain::shelf_document::query_text(&rows[0].document).as_deref(),
        Some("tag:dark AND trope:system-grind")
    );

    // Another account sees none of them: the page is scoped, and a leaked shelf would be a leaked taste.
    let other = uuid::Uuid::new_v4().to_string();
    exec(&db,
        "INSERT INTO accounts (id, email, status, age_state, created_at, updated_at) VALUES (?, ?, 'active', 'adult', ?, ?)",
        "INSERT INTO accounts (id, email, status, age_state, created_at, updated_at) VALUES ($1::uuid, $2, 'active', 'adult', $3, $4)",
        &[&other, &format!("{}@example.invalid", other), T0, T0]).await;
    let theirs = darkhold_db::shelf_document::shelves_for_account(&db, &other)
        .await
        .expect("rows");
    assert!(
        theirs.is_empty(),
        "another account's shelves are not this account's"
    );
}
