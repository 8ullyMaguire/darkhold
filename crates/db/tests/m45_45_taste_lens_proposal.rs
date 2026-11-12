//! M45-45 step 3 — a lens change through a vote, proved on a migrated database.
//!
//! Under `[taste] lens_owner = "vote"` a direct write is refused (see `routes::discovery::lens_write_allowed`), so this
//! is the only path by which the lens can change. Three things have to be true together, and each is a separate
//! assertion because they fail differently:
//!
//! * **a second vote replaces the first** — a reader who changes their mind must not be worth two, and the primary key,
//!   not a handler check, is what enforces it;
//! * **no votes is not a rejection** — a proposal nobody looked at stays `open`, because "0%" would read as a decision
//!   the instance never took;
//! * **a passing proposal writes `changed_by = proposal:<id>`** — the history row's job is to say *why* the version
//!   changed, and an account id cannot hold that.

use darkhold_db::instance_taste_settings::{self, StoredTasteSettings, TasteSettingsUpdate};
use darkhold_db::taste_lens_proposals::{self, Proposal};
use std::path::PathBuf;

use test_support::TestDb;

/// A real UUID, because `updated_by` is a UUID column on PostgreSQL and `operator-1` is not storable there.
const OPERATOR: &str = "00000000-0000-0000-0000-0000000000aa";

fn scratch(tag: &str) -> PathBuf {
    let dir = PathBuf::from(std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".to_string()))
        .join(format!("darkhold-m45-45-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn update(gravity: i64) -> TasteSettingsUpdate {
    TasteSettingsUpdate {
        gravity_strength: gravity,
        signal_weight_mode: "balanced".to_string(),
        admin_weight: 5000,
        diversity_injection_percent: 2500,
    }
}

/// A proposal that has already closed. `decide_if_due` compares `closes_at` as a string against a caller-supplied
/// `now`, so a past date is all that is needed to make it due without waiting.
async fn closed_proposal(db: &darkhold_db::Database, threshold: i64) -> Proposal {
    taste_lens_proposals::open(
        db,
        &update(3000),
        OPERATOR,
        "2026-01-01T00:00:00Z",
        threshold,
    )
    .await
    .expect("open a proposal")
}

#[tokio::test]
async fn a_passing_proposal_writes_the_lens_and_says_the_proposal_did_it() {
    let harness = TestDb::connect_with_dir("passing", &scratch("passing")).await;
    let db = harness.db();
    let proposal = closed_proposal(db, 60).await;

    taste_lens_proposals::cast_vote(db, &proposal.id, "reader-a", true)
        .await
        .expect("vote");
    let tally = taste_lens_proposals::cast_vote(db, &proposal.id, "reader-b", true)
        .await
        .expect("vote");
    assert_eq!((tally.yes, tally.no), (2, 0), "two yes votes");

    let decided = taste_lens_proposals::decide_if_due(db, &proposal.id, "2026-01-02T00:00:00Z")
        .await
        .expect("decide")
        .expect("the proposal exists");
    assert_eq!(
        decided.state, "passed",
        "100% of two votes carries a 60% threshold"
    );

    let stored: StoredTasteSettings = instance_taste_settings::read(db)
        .await
        .expect("read settings")
        .expect("the passing proposal wrote a row");
    assert_eq!(
        stored.gravity_strength, 3000,
        "the knobs the proposal carried are the ones in force"
    );
    assert_eq!(
        stored.updated_by.as_deref(),
        Some(OPERATOR),
        "the applied version names the operator who opened the proposal, because the account column is a UUID"
    );
    let history = instance_taste_settings::history(db, 1)
        .await
        .expect("read history");
    let latest = history.first().expect("a version was written");
    assert_eq!(
        latest.cause,
        format!("proposal:{}", proposal.id),
        "the history must say the proposal caused this change, not only who pressed the button"
    );
}

#[tokio::test]
async fn a_second_vote_from_one_account_replaces_the_first() {
    let harness = TestDb::connect_with_dir("replace", &scratch("replace")).await;
    let db = harness.db();
    let proposal = closed_proposal(db, 60).await;

    taste_lens_proposals::cast_vote(db, &proposal.id, "reader-a", true)
        .await
        .expect("first vote");
    let tally = taste_lens_proposals::cast_vote(db, &proposal.id, "reader-a", false)
        .await
        .expect("changed mind");

    assert_eq!(
        (tally.yes, tally.no),
        (0, 1),
        "one account is one vote, whichever way it last went"
    );
    let decided = taste_lens_proposals::decide_if_due(db, &proposal.id, "2026-01-02T00:00:00Z")
        .await
        .expect("decide")
        .expect("the proposal exists");
    assert_eq!(
        decided.state, "rejected",
        "0% against a 60% threshold does not pass"
    );
    assert!(
        instance_taste_settings::read(db)
            .await
            .expect("read settings")
            .is_none(),
        "a rejected proposal writes nothing: the lens is not a place to accumulate attempts"
    );
}

#[tokio::test]
async fn a_proposal_nobody_voted_on_stays_open() {
    let harness = TestDb::connect_with_dir("nobody", &scratch("nobody")).await;
    let db = harness.db();
    let proposal = closed_proposal(db, 50).await;

    let decided = taste_lens_proposals::decide_if_due(db, &proposal.id, "2026-01-02T00:00:00Z")
        .await
        .expect("decide")
        .expect("the proposal exists");

    assert_eq!(
        decided.state, "open",
        "no votes is an absence of a decision, not a rejection: it must stay open for an operator to see"
    );
    assert!(
        decided.decided_at.is_none(),
        "and it must not claim a decision time"
    );
}

#[tokio::test]
async fn a_proposal_that_has_not_closed_is_left_alone() {
    let harness = TestDb::connect_with_dir("not-due", &scratch("not-due")).await;
    let db = harness.db();
    let proposal =
        taste_lens_proposals::open(db, &update(1000), OPERATOR, "2099-01-01T00:00:00Z", 50)
            .await
            .expect("open a future-dated proposal");
    taste_lens_proposals::cast_vote(db, &proposal.id, "reader-a", true)
        .await
        .expect("vote");

    let decided = taste_lens_proposals::decide_if_due(db, &proposal.id, "2026-01-02T00:00:00Z")
        .await
        .expect("decide")
        .expect("the proposal exists");

    assert_eq!(
        decided.state, "open",
        "a unanimous vote cannot pass a proposal before it closes"
    );
    assert!(
        instance_taste_settings::read(db)
            .await
            .expect("read settings")
            .is_none(),
        "and nothing is written early"
    );
}
