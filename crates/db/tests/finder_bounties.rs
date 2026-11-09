//! M45-34 — bounties and their escrow, on both engines.
//!
//! The credits are the feature: a bounty that can be answered without being payable, or paid twice, or paid to
//! the asker, is worse than no bounty at all.

use darkhold_db::finder::{self as store, Accepted, Created, NewBounty};
use darkhold_db::Database;
use darkhold_domain::economy::TxnType;

fn scratch_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-finder-{tag}-{}-{:?}",
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
                "lh_test_finder_{}_{}",
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

struct Fixture {
    asker: String,
    finder: String,
    work: String,
}

async fn fixture(db: &Database, credits: i64) -> Fixture {
    let make = |label: &str| {
        (
            uuid::Uuid::new_v4().to_string(),
            uuid::Uuid::new_v4().to_string(),
            label.to_owned(),
        )
    };
    let (asker, asker_pseud, asker_label) = make("asker");
    let (finder, finder_pseud, finder_label) = make("finder");
    for (account, pseud, label) in [
        (asker.clone(), asker_pseud.clone(), asker_label),
        (finder.clone(), finder_pseud.clone(), finder_label),
    ] {
        exec(db, &format!(
            "INSERT INTO accounts (id, email, created_at, updated_at) VALUES ('{account}', '{label}-{account}@x.test', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")).await;
        exec(db, &format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at) VALUES ('{pseud}', '{account}', '{label}{account}', '{label}', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")).await;
    }
    let work = uuid::Uuid::new_v4().to_string();
    exec(db, &format!(
        "INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at) VALUES ('{work}', '{asker_pseud}', 'The remembered fic', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")).await;
    darkhold_db::economy::post_transaction(
        db,
        TxnType::Grant,
        &format!("grant:{asker}"),
        "test grant",
        &[(asker.clone(), "earned".to_owned(), credits)],
    )
    .await
    .expect("grant");
    Fixture {
        asker,
        finder,
        work,
    }
}

fn new_bounty(budget: i64, closes_at: &str) -> NewBounty {
    NewBounty {
        description: "Naruto fic read around 2019: Sakura learns a seal, there is a broken tanto in chapter 3,                       and the ending is on a rooftop in the rain."
            .to_owned(),
        fandom: Some("Naruto".to_owned()),
        budget,
        closes_at: closes_at.to_owned(),
    }
}

fn soon(days: i64) -> String {
    (time::OffsetDateTime::now_utc() + time::Duration::days(days))
        .format(&time::format_description::well_known::Rfc3339)
        .expect("format")
}

#[tokio::test]
async fn a_bounty_reserves_its_budget_and_pays_one_answer() {
    let (db, dir) = database("pay").await;
    let fx = fixture(&db, 40).await;

    assert_eq!(
        store::available_credits(&db, &fx.asker)
            .await
            .expect("available"),
        40
    );
    let Created::Bounty(bounty) = store::create(&db, &fx.asker, &new_bounty(25, &soon(7)))
        .await
        .expect("create")
    else {
        panic!("expected a bounty");
    };
    assert_eq!(bounty.state.as_str(), "open");
    assert!(bounty.hold_id.is_some(), "the promise is backed by a hold");
    assert_eq!(
        store::available_credits(&db, &fx.asker)
            .await
            .expect("available"),
        15,
        "the budget is reserved, not merely promised"
    );

    let proposal = store::propose(
        &db,
        &bounty.id,
        &fx.finder,
        &fx.work,
        "I think it is this one",
    )
    .await
    .expect("propose")
    .unwrap_or_else(|reason| panic!("{reason}"));
    // `proposals().len()` rather than a `proposal_count` helper: the helper was a second way to answer the same question, its
    // doc described something the signature could not do, and the answer the detail route serves is this list.
    assert_eq!(
        store::proposals(&db, &bounty.id).await.expect("proposals").len(),
        1
    );

    let Accepted::Paid { bounty, .. } = store::accept(&db, &bounty.id, &proposal.id, &fx.asker)
        .await
        .expect("accept")
    else {
        panic!("expected a payment");
    };
    assert_eq!(bounty.state.as_str(), "accepted");
    assert_eq!(
        bounty.accepted_proposal_id.as_deref(),
        Some(proposal.id.as_str())
    );
    assert_eq!(
        store::available_credits(&db, &fx.asker)
            .await
            .expect("available"),
        15,
        "the capture consumes the hold, so nothing stays reserved"
    );
    assert_eq!(
        store::available_credits(&db, &fx.finder)
            .await
            .expect("available"),
        25,
        "the finder is paid the budget"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn nobody_pays_themselves_and_a_closed_bounty_takes_no_answers() {
    let (db, dir) = database("rules").await;
    let fx = fixture(&db, 40).await;
    let Created::Bounty(bounty) = store::create(&db, &fx.asker, &new_bounty(10, &soon(3)))
        .await
        .expect("create")
    else {
        panic!("expected a bounty");
    };

    // The asker answering their own bounty is refused, with the sentence saying what to do instead.
    let own = store::propose(&db, &bounty.id, &fx.asker, &fx.work, "me")
        .await
        .expect("propose");
    assert!(own.is_err(), "an asker cannot answer their own bounty");
    assert!(own.unwrap_err().contains("your own"));

    // The same person naming the same work twice is one answer.
    let first = store::propose(&db, &bounty.id, &fx.finder, &fx.work, "this")
        .await
        .expect("propose");
    assert!(first.is_ok(), "{first:?}");
    let repeat = store::propose(&db, &bounty.id, &fx.finder, &fx.work, "this again")
        .await
        .expect("propose");
    assert!(repeat.is_err(), "a duplicate answer is refused");

    // Cancel releases the hold, and a cancelled bounty accepts nothing.
    let proposal = first.unwrap();
    let cancelled = store::cancel(&db, &bounty.id, &fx.asker)
        .await
        .expect("cancel")
        .expect("cancelled bounty");
    assert_eq!(cancelled.state.as_str(), "cancelled");
    assert_eq!(
        store::available_credits(&db, &fx.asker)
            .await
            .expect("available"),
        40,
        "cancelling returns the credits"
    );
    match store::accept(&db, &bounty.id, &proposal.id, &fx.asker)
        .await
        .expect("accept")
    {
        Accepted::Refused(reason) => assert!(reason.contains("cancelled"), "{reason}"),
        other => panic!("a cancelled bounty must not pay: {other:?}"),
    }
    assert!(
        store::cancel(&db, &bounty.id, &fx.finder)
            .await
            .expect("cancel")
            .is_none(),
        "only the asker cancels"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn two_accepts_produce_one_winner_and_one_payment() {
    let (db, dir) = database("race").await;
    let fx = fixture(&db, 100).await;
    let Created::Bounty(bounty) = store::create(&db, &fx.asker, &new_bounty(30, &soon(5)))
        .await
        .expect("create")
    else {
        panic!("expected a bounty");
    };
    let second_work = uuid::Uuid::new_v4().to_string();
    exec(&db, &format!(
        "INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at) SELECT '{second_work}', owner_pseud_id, 'Other candidate', '2026-01-02T00:00:00Z', '2026-01-02T00:00:00Z' FROM works WHERE id = '{0}'", fx.work)).await;

    let one = store::propose(&db, &bounty.id, &fx.finder, &fx.work, "a")
        .await
        .expect("propose")
        .expect("ok");
    let two = store::propose(&db, &bounty.id, &fx.finder, &second_work, "b")
        .await
        .expect("propose")
        .expect("ok");

    let first = store::accept(&db, &bounty.id, &one.id, &fx.asker)
        .await
        .expect("accept");
    assert!(matches!(first, Accepted::Paid { .. }), "{first:?}");
    let second = store::accept(&db, &bounty.id, &two.id, &fx.asker)
        .await
        .expect("accept");
    match second {
        Accepted::Refused(reason) => assert!(
            reason.contains("accepted") || reason.contains("closed"),
            "{reason}"
        ),
        other => panic!("only one accept pays: {other:?}"),
    }
    assert_eq!(
        store::available_credits(&db, &fx.finder)
            .await
            .expect("available"),
        30,
        "paid exactly once"
    );
    let states: Vec<String> = store::proposals(&db, &bounty.id)
        .await
        .expect("proposals")
        .into_iter()
        .map(|row| row.state)
        .collect();
    assert!(
        states.contains(&"accepted".to_owned()) && states.contains(&"rejected".to_owned()),
        "{states:?}"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_bounty_cannot_promise_more_than_the_asker_has_and_expiry_releases_the_rest() {
    let (db, dir) = database("limits").await;
    let fx = fixture(&db, 5).await;

    let too_much = store::create(&db, &fx.asker, &new_bounty(25, &soon(7)))
        .await
        .expect("create");
    match too_much {
        Created::Refused(reason) => assert!(
            reason.contains("available") && reason.contains("5"),
            "{reason}"
        ),
        other => panic!("an unfunded bounty must be refused: {other:?}"),
    }

    // A short description is refused before any hold is taken.
    let mut thin = new_bounty(2, &soon(7));
    thin.description = "a fic pls".to_owned();
    match store::create(&db, &fx.asker, &thin).await.expect("create") {
        Created::Refused(reason) => assert!(reason.contains("describe"), "{reason}"),
        other => panic!("{other:?}"),
    }

    let Created::Bounty(bounty) = store::create(&db, &fx.asker, &new_bounty(5, &soon(7)))
        .await
        .expect("create")
    else {
        panic!("expected a funded bounty");
    };
    // Wind the close date back and sweep: the credits come free again.
    exec(
        &db,
        &format!(
            "UPDATE finder_bounties SET closes_at = '2020-01-01T00:00:00Z' WHERE id = '{}'",
            bounty.id
        ),
    )
    .await;
    let expired = store::sweep_expired(&db).await.expect("sweep");
    assert_eq!(expired, 1);
    assert_eq!(
        store::available_credits(&db, &fx.asker)
            .await
            .expect("available"),
        5,
        "an expired bounty returns its credits"
    );
    assert_eq!(
        store::sweep_expired(&db).await.expect("sweep again"),
        0,
        "idempotent"
    );
    assert!(store::open_bounties(&db, None)
        .await
        .expect("open")
        .is_empty());

    let _ = std::fs::remove_dir_all(dir);
}
