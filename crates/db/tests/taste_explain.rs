//! M45-70 — the explanation is read out of the acts, so this test performs the acts.

use std::time::Duration;

use darkhold_db::{Backend, Database};
use darkhold_domain::taste_explain::{explain, Event, Signal};

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
async fn the_explanation_is_the_acts_the_reader_performed_and_their_switches() {
    let db = connect("acts").await;
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

    // Two works: one about necromancy (finished, 5 stars), one about fluff (1 star, dropped).
    for (work, title) in [("w-dark", "Necromancer"), ("w-fluff", "Cotton Candy")] {
        exec(&db,
            "INSERT INTO works (id, owner_pseud_id, title, summary, created_at, updated_at) VALUES (?, ?, ?, '', ?, ?)",
            "INSERT INTO works (id, owner_pseud_id, title, summary, created_at, updated_at) VALUES (?::uuid, ?::uuid, ?, '', ?::timestamptz, ?::timestamptz)",
            &[work, &pseud, title, T0, T0]).await;
    }
    for (node, canon) in [("t1", "necromancy"), ("t2", "fluff"), ("t3", "system")] {
        exec(&db,
            "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at) VALUES (?, 'tag', ?, ?, ?)",
            "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at) VALUES (?::uuid, 'tag', ?, ?, ?::timestamptz)",
            &[node, canon, canon, T0]).await;
    }
    for (work, node) in [("w-dark", "t1"), ("w-dark", "t3"), ("w-fluff", "t2")] {
        exec(&db,
            "INSERT INTO work_tags (work_id, node_id, added_at) VALUES (?, ?, ?)",
            "INSERT INTO work_tags (work_id, node_id, added_at) VALUES (?::uuid, ?::uuid, ?::timestamptz)",
            &[work, node, T0]).await;
    }

    // The acts: finished the dark work and rated it 5; dropped the fluffy one and rated it 1.
    for (work, status) in [("w-dark", "finished"), ("w-fluff", "dropped")] {
        exec(&db,
            "INSERT INTO reading_status (id, account_id, subject_type, subject_id, status, updated_at) VALUES (?, ?, 'work', ?, ?, ?)",
            "INSERT INTO reading_status (id, account_id, subject_type, subject_id, status, updated_at) VALUES (gen_random_uuid(), ?::uuid, 'work', ?::uuid, ?, ?::timestamptz)",
            &[&uuid::Uuid::new_v4().to_string(), &account, work, status, T0]).await;
    }
    for (work, stars) in [("w-dark", "5"), ("w-fluff", "1")] {
        exec(&db,
            "INSERT INTO rating (id, account_id, pseud_id, work_id, stars, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
            "INSERT INTO rating (id, account_id, pseud_id, work_id, stars, created_at, updated_at) VALUES (gen_random_uuid(), ?::uuid, ?::uuid, ?::uuid, ?, ?::timestamptz, ?::timestamptz)",
            &[&uuid::Uuid::new_v4().to_string(), &account, &pseud, work, stars, T0, T0]).await;
    }

    let events = darkhold_db::taste_explain::events(&db, &account)
        .await
        .expect("events");
    let explained = explain(&events, &[]);
    let necromancy = explained
        .iter()
        .find(|t| t.tag == "necromancy")
        .expect("necromancy");
    assert_eq!(
        necromancy.weight, 5.0,
        "finished (3) + five stars (2) on one work, and both signals count"
    );
    let fluff = explained.iter().find(|t| t.tag == "fluff").expect("fluff");
    assert_eq!(
        fluff.weight, -3.0,
        "one star (-2) plus dropped (-1): dislike is evidence too"
    );

    // The switch: turning `finished` off must move weight into `suppressed`, not delete the tag.
    darkhold_db::taste_explain::set_signal(&db, &account, "finished", false, T0)
        .await
        .expect("off");
    let disabled = darkhold_db::taste_explain::disabled_signals(&db, &account)
        .await
        .expect("prefs");
    assert_eq!(disabled, vec!["finished".to_string()]);
    let off: Vec<Signal> = disabled
        .iter()
        .filter_map(|key| Signal::parse(key))
        .collect();
    let explained = explain(&events, &off);
    let necromancy = explained
        .iter()
        .find(|t| t.tag == "necromancy")
        .expect("necromancy");
    assert_eq!(necromancy.weight, 2.0, "only the rating counts now");
    assert_eq!(
        necromancy.suppressed, 3.0,
        "and the reader can see what they turned off"
    );

    // A different reader's explanation is their own.
    let other = uuid::Uuid::new_v4().to_string();
    assert!(
        darkhold_db::taste_explain::events(&db, &other)
            .await
            .expect("events")
            .is_empty(),
        "no acts, no explanation"
    );
    let _ = Event {
        work: String::new(),
        tag: String::new(),
        signal: Signal::Finished,
    };
}
