//! M69-07 — the evacuation store, on whichever engine `DARKHOLD_TEST_PG_URL` names.
//!
//! The claims that belong to the store rather than the planner:
//!
//! * one work, one placement per notice — the database refuses a second row, not just the planner;
//! * **one work, one home across every notice**: a second evacuation that would hand the same work to a different peer is
//!   refused, and the refusal names the peer that already has it;
//! * one open announcement at a time;
//! * closing a notice keeps the receipt.

use darkhold_db::{evacuation, Database};
use darkhold_domain::evacuation::{plan, WorkNeed};
use darkhold_domain::mirroring::Eligible;

const NOW: &str = "2026-11-07T00:00:00Z";

fn make_config(url: String) -> darkhold_db::DatabaseConfig {
    let mut config = darkhold_db::DatabaseConfig::new(url);
    config.max_connections = 2;
    config.acquire_timeout = std::time::Duration::from_secs(60);
    config
}

async fn connect() -> Database {
    let Ok(admin) = std::env::var("DARKHOLD_TEST_PG_URL") else {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "darkhold-evac-{}-{}",
            uuid::Uuid::new_v4().simple(),
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let db = Database::connect(&make_config(format!(
            "sqlite://{}/darkhold.sqlite?mode=rwc",
            dir.display()
        )))
        .await
        .expect("connect sqlite");
        db.migrate().await.expect("migrate");
        return db;
    };
    let name = format!("lh_evac_{}", uuid::Uuid::new_v4().simple());
    let admin_db = Database::connect(&make_config(admin.clone())).await.expect("admin");
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(admin_db.postgres_pool().expect("postgres handle"))
        .await
        .expect("create database");
    let url = admin
        .rsplit_once('/')
        .map(|(prefix, _)| format!("{prefix}/{name}"))
        .expect("a database url has a path");
    let db = Database::connect(&make_config(url)).await.expect("connect");
    db.migrate().await.expect("migrate");
    db
}

fn peers(similarities: [f64; 2]) -> Vec<Eligible> {
    vec![
        Eligible {
            instance_id: "https://a.example".to_owned(),
            similarity: similarities[0],
            measured: true,
        },
        Eligible {
            instance_id: "https://b.example".to_owned(),
            similarity: similarities[1],
            measured: true,
        },
    ]
}

fn works(count: usize) -> Vec<WorkNeed> {
    (0..count)
        .map(|index| WorkNeed {
            work_id: format!("work-{index}"),
            title: format!("Work {index}"),
        })
        .collect()
}

#[tokio::test]
async fn one_work_gets_one_home_and_a_second_evacuation_cannot_move_it() {
    let db = connect().await;
    let db = &db;

    let first_plan = plan(&works(4), &peers([0.9, 0.85]));
    let (notice, assignments) = evacuation::announce(
        db,
        "the host is being retired",
        "operator",
        "",
        &first_plan,
        NOW,
    )
    .await
    .expect("announce");
    assert_eq!(notice.state, "announced");
    assert_eq!(assignments.len(), 4);

    // One row per work: the unique index, not only the planner, is what makes that true.
    let duplicate = insert_duplicate_claim(db, &notice.id, "work-0", "https://b.example").await;
    assert!(
        duplicate.contains("unique") || duplicate.contains("duplicate"),
        "a second row for one work in one notice must be refused: {duplicate}"
    );

    // A second announcement is refused while one is open.
    let error = evacuation::announce(db, "again", "operator", "", &first_plan, NOW)
        .await
        .expect_err("two open announcements is two plans for one shutdown");
    assert!(error.to_string().contains("already announced"), "{error}");

    // The first accepted placement is that work's home.
    let first = assignments
        .iter()
        .find(|row| row.work_id == "work-0")
        .expect("work-0 was placed");
    let settled = evacuation::settle(db, &first.id, "accepted", "confirmed by mail", NOW)
        .await
        .expect("accept");
    assert_eq!(settled.state, "accepted");

    // A settled placement does not settle twice.
    let error = evacuation::settle(db, &first.id, "declined", "", NOW)
        .await
        .expect_err("already settled");
    assert!(error.to_string().contains("already settled"), "{error}");

    // Re-recording the SAME peer against the same work is history, not a fork.
    let receipt = evacuation::receipt(db, &notice.id).await.expect("receipt");
    assert_eq!(receipt.accepted.len(), 1);
    assert!(receipt.forks().is_empty(), "one recipient, recorded once");

    // Close, then evacuate again: the same work offered to the other peer must not be accepted.
    evacuation::close(db, "completed", NOW).await.expect("close");
    let other_peer = peers([0.0, 0.9])
        .into_iter()
        .filter(|peer| peer.instance_id != first.recipient_instance)
        .collect::<Vec<Eligible>>();
    let moved = plan(&works(1), &other_peer);
    assert_eq!(moved.placements[0].work_id, "work-0");
    let (second_notice, second) = evacuation::announce(
        db,
        "the migration was reversed",
        "operator",
        "",
        &moved,
        "2026-11-08T00:00:00Z",
    )
    .await
    .expect("announce again");
    let error = evacuation::settle(
        db,
        &second[0].id,
        "accepted",
        "we have it too",
        "2026-11-08T00:00:00Z",
    )
    .await
    .expect_err("one work gets one home, or the author's page forks");
    assert!(
        error.to_string().contains(&first.recipient_instance),
        "the refusal names the peer that already has it: {error}"
    );
    assert!(
        error.to_string().contains(&notice.id),
        "and the notice it took it in: {error}"
    );

    // Declining is always allowed, and the receipt says so.
    let declined = evacuation::settle(db, &second[0].id, "declined", "not ours", "2026-11-08T00:00:00Z")
        .await
        .expect("decline");
    assert_eq!(declined.state, "declined");
    let receipt = evacuation::receipt(db, &second_notice.id)
        .await
        .expect("receipt");
    assert_eq!(receipt.declined.len(), 1);
    assert_eq!(receipt.declined[0].work_id, "work-0");
    assert_eq!(receipt.declined[0].reason, "not ours");
}

/// Insert a second claim for a work, returning the error text. The planner would never produce one; the store's own guard is
/// what is under test.
async fn insert_duplicate_claim(
    db: &Database,
    notice_id: &str,
    work_id: &str,
    instance: &str,
) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    let sql = db.sql(
        "INSERT INTO evacuation_assignments
            (id, notice_id, work_id, title, recipient_instance, similarity_bp, state, offered_at)
         VALUES (?, ?, ?, 'duplicate', ?, 9000, 'offered', ?)",
        "INSERT INTO evacuation_assignments
            (id, notice_id, work_id, title, recipient_instance, similarity_bp, state, offered_at)
         VALUES ($1, $2, $3, 'duplicate', $4, 9000, 'offered', $5)",
    );
    let result = match db.backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(&id)
                .bind(notice_id)
                .bind(work_id)
                .bind(instance)
                .bind(NOW)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await
                .map(|_| ())
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(&sql)
                .bind(&id)
                .bind(notice_id)
                .bind(work_id)
                .bind(instance)
                .bind(NOW)
                .execute(db.postgres_pool().expect("postgres"))
                .await
                .map(|_| ())
        }
    };
    result
        .expect_err("a second row for one work in one notice violates the unique index")
        .to_string()
        .to_lowercase()
}

#[tokio::test]
async fn an_announcement_needs_a_reason_and_closing_needs_a_state_it_recognises() {
    let db = connect().await;
    let db = &db;
    let only_plan = plan(&works(1), &peers([0.9, 0.85]));
    let error = evacuation::announce(db, "   ", "operator", "", &only_plan, NOW)
        .await
        .expect_err("a reason is what a reader who was not here needs");
    assert!(error.to_string().contains("reason"), "{error}");

    let (notice, _) = evacuation::announce(db, "retiring", "operator", "", &only_plan, NOW)
        .await
        .expect("announce");
    let error = evacuation::close(db, "finished", NOW)
        .await
        .expect_err("'finished' is not a state");
    assert!(error.to_string().contains("withdrawn"), "{error}");

    // Withdrawing keeps the notice and its placements.
    let withdrawn = evacuation::close(db, "withdrawn", NOW).await.expect("withdraw");
    assert_eq!(withdrawn.state, "withdrawn");
    assert_eq!(
        evacuation::assignments_for(db, &notice.id)
            .await
            .expect("assignments")
            .len(),
        1
    );
    assert!(
        evacuation::close(db, "withdrawn", NOW).await.is_err(),
        "nothing is open to close"
    );
}
