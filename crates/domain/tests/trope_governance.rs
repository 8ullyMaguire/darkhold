//! Step 5: pure trope governance rules, borrowed from the category tally.

use darkhold_domain::trope_governance::{
    effective_status, proposal_decided, valid_slug, TropeAction,
};

#[test]
fn only_closed_lowercase_hyphenated_slugs_are_accepted() {
    assert!(valid_slug("system-progression"));
    assert!(valid_slug("a12"));
    for bad in [
        "ab",
        "System-Progression",
        "-leading",
        "trailing-",
        "double--dash",
        "with space",
        "under_score",
    ] {
        assert!(!valid_slug(bad), "accepted an unusable query token: {bad}");
    }
}

#[test]
fn two_approvals_pass_and_two_refusals_fail() {
    assert_eq!(proposal_decided(2, 0, 2), Some(true));
    assert_eq!(proposal_decided(0, 2, 2), Some(false));
}

#[test]
fn a_split_vote_stays_undecided() {
    assert_eq!(proposal_decided(1, 1, 2), None);
}

#[test]
fn expiry_boundary_is_inclusive_and_only_proposals_expire() {
    let closes = "2026-01-01T00:00:00Z";
    assert_eq!(
        effective_status("proposed", closes, "2025-12-31T23:59:59Z"),
        "proposed"
    );
    assert_eq!(
        effective_status("proposed", closes, closes),
        "expired",
        "a proposal closes at its deadline, not after it"
    );
    assert_eq!(
        effective_status("active", closes, "2027-01-01T00:00:00Z"),
        "active"
    );
}

#[test]
fn retire_action_parses_and_round_trips() {
    assert_eq!(TropeAction::parse("retire"), Some(TropeAction::Retire));
    assert_eq!(TropeAction::Retire.as_str(), "retire");
    assert_eq!(TropeAction::parse("delete"), None);
}
