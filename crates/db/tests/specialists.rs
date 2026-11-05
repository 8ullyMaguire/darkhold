//! M45-33 — specialist matchmaking in the store, on both engines.
//!
//! The properties only the store can have:
//! * an opted-in profile is the only thing that makes a candidate — a closed profile is invisible, and a
//!   profile with no row is absent rather than closed;
//! * replacing the skills replaces them (**a profile is the set a person claims now**, not a delta);
//! * two specialists accepting the same request: exactly one wins, and the loser's write changes nothing —
//!   the guard is `WHERE state = 'open'`, not a read-then-write in Rust;
//! * deleting a profile takes its skills with it (the FK, not a cleanup job).

use darkhold_db::specialist as store;
use darkhold_db::Database;
use darkhold_domain::specialist::{Capacity, Skill, State};

fn scratch_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-spec-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch");
    dir
}

async fn database(tag: &str) -> (Database, std::path::PathBuf) {
    let dir = scratch_dir(tag);
    let db = match std::env::var("DARKHOLD_TEST_PG_URL") {
        Ok(admin_url) if !admin_url.is_empty() => {
            static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let name = format!(
                "lh_test_spec_{}_{}",
                std::process::id(),
                COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            );
            test_support::empty_postgres(&admin_url, &name)
                .await
                .expect("empty PostgreSQL database")
        }
        _ => {
            let config = darkhold_db::DatabaseConfig::new(format!(
                "sqlite://{}/darkhold.sqlite?mode=rwc",
                dir.display()
            ));
            Database::connect(&config).await.expect("connect")
        }
    };
    db.migrate().await.expect("migrate");
    (db, dir)
}

async fn exec(db: &Database, statement: &str) {
    let result = match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query(statement)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await
            .map(|_| ()),
        darkhold_db::Backend::Postgres => sqlx::query(statement)
            .execute(db.postgres_pool().expect("postgres"))
            .await
            .map(|_| ()),
    };
    result.unwrap_or_else(|error| panic!("{statement}\n{error}"));
}

/// An account with one pseud per handle, and a work owned by the first.
async fn people(db: &Database, handles: &[&str]) -> (Vec<String>, String) {
    let account = uuid::Uuid::new_v4().to_string();
    exec(
        db,
        &format!(
            "INSERT INTO accounts (id, email, created_at, updated_at) \
             VALUES ('{account}', 'spec-{account}@x.test', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        ),
    )
    .await;
    let mut pseuds = Vec::new();
    for handle in handles {
        let pseud = uuid::Uuid::new_v4().to_string();
        exec(
            db,
            &format!(
                "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at) \
                 VALUES ('{pseud}', '{account}', '{handle}{pseud}', '{handle}', \
                         '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
            ),
        )
        .await;
        pseuds.push(pseud);
    }
    let work = uuid::Uuid::new_v4().to_string();
    exec(
        db,
        &format!(
            "INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at) \
             VALUES ('{work}', '{}', 'A work', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            pseuds[0]
        ),
    )
    .await;
    (pseuds, work)
}

#[tokio::test]
async fn a_profile_is_opt_in_replaced_wholesale_and_absent_when_deleted() {
    let (db, dir) = database("profile").await;
    let (pseuds, _work) = people(&db, &["writer", "beta"]).await;
    let beta = &pseuds[1];

    assert!(
        store::profile(&db, beta).await.expect("profile").is_none(),
        "no row means absent, not closed"
    );

    let written = store::upsert_profile(
        &db,
        beta,
        &[Skill::Beta, Skill::Britpick],
        &["naruto".to_owned(), "Harry Potter".to_owned()],
        Capacity::Open,
        "line edits, British English",
    )
    .await
    .expect("upsert");
    assert_eq!(written.skills, vec![Skill::Beta, Skill::Britpick]);
    assert_eq!(written.fandoms, vec!["naruto", "Harry Potter"]);

    // Replacing with one skill drops the other: the profile is a claim about now.
    let replaced =
        store::upsert_profile(&db, beta, &[Skill::Sensitivity], &[], Capacity::Closed, "")
            .await
            .expect("upsert");
    assert_eq!(replaced.skills, vec![Skill::Sensitivity]);
    assert!(replaced.fandoms.is_empty(), "empty means any fandom");
    assert_eq!(replaced.capacity, Capacity::Closed);

    assert!(store::delete_profile(&db, beta).await.expect("delete"));
    assert!(store::profile(&db, beta).await.expect("profile").is_none());
    assert!(
        !store::delete_profile(&db, beta)
            .await
            .expect("delete again"),
        "deleting twice is false, not an error"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn candidates_are_the_open_profiles_for_the_skill_with_their_load() {
    let (db, dir) = database("candidates").await;
    let (pseuds, work) = people(&db, &["writer", "beta", "closed", "other"]).await;
    let (writer, beta, closed, other) = (&pseuds[0], &pseuds[1], &pseuds[2], &pseuds[3]);

    store::upsert_profile(
        &db,
        beta,
        &[Skill::Beta],
        &["naruto".to_owned()],
        Capacity::Open,
        "",
    )
    .await
    .expect("beta");
    store::upsert_profile(&db, closed, &[Skill::Beta], &[], Capacity::Closed, "")
        .await
        .expect("closed");
    store::upsert_profile(&db, other, &[Skill::Britpick], &[], Capacity::Open, "")
        .await
        .expect("other");

    let beta_candidates = store::candidates(&db, Skill::Beta)
        .await
        .expect("candidates");
    assert_eq!(
        beta_candidates.len(),
        1,
        "only the open beta: {beta_candidates:?}"
    );
    assert_eq!(beta_candidates[0].pseud_id, *beta);
    assert_eq!(beta_candidates[0].fandoms, vec!["naruto"]);
    assert_eq!(beta_candidates[0].live_engagements, 0);

    // One live engagement later, the load is visible.
    let request = store::create_request(
        &db,
        &uuid::Uuid::new_v4().to_string(),
        &work,
        writer,
        Skill::Beta,
        "naruto",
        "please",
    )
    .await
    .expect("request");
    assert!(
        store::set_state(&db, &request.id, State::Open, State::Matched, Some(beta))
            .await
            .expect("accept")
    );
    let after = store::candidates(&db, Skill::Beta)
        .await
        .expect("candidates");
    assert_eq!(after[0].live_engagements, 1, "{after:?}");

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn only_one_specialist_can_accept_the_same_request() {
    let (db, dir) = database("race").await;
    let (pseuds, work) = people(&db, &["writer", "first", "second"]).await;
    let (writer, first, second) = (&pseuds[0], &pseuds[1], &pseuds[2]);

    let request = store::create_request(
        &db,
        &uuid::Uuid::new_v4().to_string(),
        &work,
        writer,
        Skill::Beta,
        "",
        "two can try",
    )
    .await
    .expect("request");

    assert!(
        store::set_state(&db, &request.id, State::Open, State::Matched, Some(first))
            .await
            .expect("first"),
        "the first write wins"
    );
    assert!(
        !store::set_state(&db, &request.id, State::Open, State::Matched, Some(second))
            .await
            .expect("second"),
        "the second write changes nothing: the request is no longer open"
    );
    let stored = store::request(&db, &request.id)
        .await
        .expect("request")
        .expect("row");
    assert_eq!(stored.state, State::Matched);
    assert_eq!(
        stored.matched_pseud_id.as_deref(),
        Some(first.as_str()),
        "the winner owns the match"
    );
    assert!(stored.matched_at.is_some());

    // Completing records a second timestamp and does not clear the first.
    assert!(
        store::set_state(&db, &request.id, State::Matched, State::Completed, None)
            .await
            .expect("complete")
    );
    let done = store::request(&db, &request.id)
        .await
        .expect("request")
        .expect("row");
    assert_eq!(done.state, State::Completed);
    assert!(done.completed_at.is_some());

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn open_requests_are_browsable_by_skill_and_scoped_by_state() {
    let (db, dir) = database("browse").await;
    let (pseuds, work) = people(&db, &["writer", "beta"]).await;
    let (writer, beta) = (&pseuds[0], &pseuds[1]);

    let open = store::create_request(
        &db,
        &uuid::Uuid::new_v4().to_string(),
        &work,
        writer,
        Skill::Beta,
        "",
        "open one",
    )
    .await
    .expect("request");
    store::create_request(
        &db,
        &uuid::Uuid::new_v4().to_string(),
        &work,
        writer,
        Skill::Sensitivity,
        "",
        "wrong skill",
    )
    .await
    .expect("request");

    let browsable = store::open_requests(&db, Skill::Beta).await.expect("open");
    assert_eq!(browsable.len(), 1, "{browsable:?}");
    assert_eq!(browsable[0].id, open.id);

    store::set_state(&db, &open.id, State::Open, State::Withdrawn, None)
        .await
        .expect("withdraw");
    assert!(
        store::open_requests(&db, Skill::Beta)
            .await
            .expect("open")
            .is_empty(),
        "a withdrawn request is not browsable"
    );
    assert_eq!(
        store::requests_for_work(&db, &work)
            .await
            .expect("for work")
            .len(),
        2,
        "the work still remembers both requests, whatever their state"
    );
    let _ = beta;
    let _ = std::fs::remove_dir_all(dir);
}
