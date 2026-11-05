//! M45-71 — the brief reads fandoms and tags apart, from the work a shelf row points at.

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

const T0: &str = "2026-01-01T00:00:00Z";

#[tokio::test]
async fn a_shelf_row_yields_its_fandoms_apart_from_its_tags() {
    let db = connect("facts").await;
    let account = uuid::Uuid::new_v4().to_string();
    let pseud = uuid::Uuid::new_v4().to_string();
    exec(&db,
        "INSERT INTO accounts (id, email, status, age_state, created_at, updated_at) VALUES (?, ?, 'active', 'adult', ?, ?)",
        "INSERT INTO accounts (id, email, status, age_state, created_at, updated_at) VALUES (?::uuid, ?, 'active', 'adult', ?::timestamptz, ?::timestamptz)",
        &[&account, &format!("{}@example.invalid", account), T0, T0]).await;
    exec(&db,
        "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at) VALUES (?, ?, 'reader', 'Reader', ?, ?)",
        "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at) VALUES (?::uuid, ?::uuid, 'reader', 'Reader', ?::timestamptz, ?::timestamptz)",
        &[&pseud, &account, T0, T0]).await;
    exec(&db,
        "INSERT INTO works (id, owner_pseud_id, title, summary, created_at, updated_at) VALUES ('w1', ?, 'A Gamer Guide', '', ?, ?)",
        "INSERT INTO works (id, owner_pseud_id, title, summary, created_at, updated_at) VALUES ('w1'::uuid, ?::uuid, 'A Gamer Guide', '', ?::timestamptz, ?::timestamptz)",
        &[&pseud, T0, T0]).await;
    for (node, kind, canon) in [
        ("f1", "fandom", "Harry Potter"),
        ("t1", "tag", "dark protagonist"),
        ("t2", "tag", "system"),
        ("c1", "character", "Tom Riddle"),
    ] {
        exec(&db,
            "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at) VALUES (?, ?, ?, ?, ?)",
            "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at) VALUES (?::uuid, ?, ?, ?, ?::timestamptz)",
            &[node, kind, canon, canon, T0]).await;
        exec(&db,
            "INSERT INTO work_tags (work_id, node_id, added_at) VALUES ('w1', ?, ?)",
            "INSERT INTO work_tags (work_id, node_id, added_at) VALUES ('w1'::uuid, ?::uuid, ?::timestamptz)",
            &[node, T0]).await;
    }
    exec(&db,
        "INSERT INTO library_items (id, account_id, work_id, source_key, source_work_key, title, author_text, summary,
             word_count, status, source_url, source_updated_at, last_synced_at, created_at, updated_at, likes)
         VALUES ('li1', ?, 'w1', 'sb', 'k1', 'A Gamer Guide', 'Shiro', '', 20000, 'ongoing', 'https://example.invalid/w', ?, ?, ?, ?, 5)",
        "INSERT INTO library_items (id, account_id, work_id, source_key, source_work_key, title, author_text, summary,
             word_count, status, source_url, source_updated_at, last_synced_at, created_at, updated_at, likes)
         VALUES ('li1'::uuid, ?::uuid, 'w1'::uuid, 'sb', 'k1', 'A Gamer Guide', 'Shiro', '', 20000, 'ongoing',
                 'https://example.invalid/w', ?::timestamptz, ?::timestamptz, ?::timestamptz, ?::timestamptz, 5)",
        &[&account, T0, T0, T0, T0]).await;

    let facts = darkhold_db::taste_brief::facts_for_item(&db, "li1")
        .await
        .expect("facts");
    assert_eq!(
        facts.fandoms,
        vec!["Harry Potter".to_string()],
        "the fandom is its own list"
    );
    assert_eq!(
        facts.tags,
        vec![
            "Tom Riddle".to_string(),
            "dark protagonist".to_string(),
            "system".to_string()
        ],
        "characters and tropes share the tag list: the brief is about vocabulary"
    );

    // A row with no work has no facts, which is the honest answer rather than an empty-looking one.
    exec(&db,
        "INSERT INTO library_items (id, account_id, work_id, source_key, source_work_key, title, author_text, summary,
             word_count, status, source_url, source_updated_at, last_synced_at, created_at, updated_at, likes)
         VALUES ('li2', ?, NULL, 'sb', 'k2', 'Unlinked', '', '', 1, 'ongoing', 'https://example.invalid/x', ?, ?, ?, ?, 0)",
        "INSERT INTO library_items (id, account_id, work_id, source_key, source_work_key, title, author_text, summary,
             word_count, status, source_url, source_updated_at, last_synced_at, created_at, updated_at, likes)
         VALUES ('li2'::uuid, ?::uuid, NULL, 'sb', 'k2', 'Unlinked', '', '', 1, 'ongoing', 'https://example.invalid/x',
                 ?::timestamptz, ?::timestamptz, ?::timestamptz, ?::timestamptz, 0)",
        &[&account, T0, T0, T0, T0]).await;
    let bare = darkhold_db::taste_brief::facts_for_item(&db, "li2")
        .await
        .expect("facts");
    assert!(bare.fandoms.is_empty() && bare.tags.is_empty());
}
