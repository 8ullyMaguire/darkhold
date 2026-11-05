//! M70-01 — the label freezes, the state is the absence of an owner, and there is no way back.

use darkhold_domain::orphan::{
    freeze, is_orphaned, may_reclaim, FrozenLabel, Refusal, ORPHAN_SUFFIX,
};

#[test]
fn the_label_freezes_the_pseud_it_was_published_under() {
    let label = freeze("hollowgast", true, false).expect("may orphan");
    assert_eq!(label.as_str(), format!("hollowgast{ORPHAN_SUFFIX}"));
    // Not the account login: a reader knows the pseud, and orphaning one account must not relabel five works.
    let other = freeze("  Kestrel  ", true, false).expect("may orphan");
    assert_eq!(
        other.as_str(),
        format!("Kestrel{ORPHAN_SUFFIX}"),
        "trimmed, never padded"
    );
}

#[test]
fn an_already_orphaned_work_is_refused_rather_than_relabelled_twice() {
    assert_eq!(
        freeze("hollowgast", false, true),
        Err(Refusal::AlreadyAnOrphan),
        "appending the suffix twice would imply a second owner was detached"
    );
    // The check order matters: already-orphaned wins over no-owner, because that is the true reason.
    assert_eq!(freeze("x", false, true), Err(Refusal::AlreadyAnOrphan));
}

#[test]
fn a_work_with_no_owner_and_an_empty_label_are_both_refused() {
    assert_eq!(freeze("hollowgast", false, false), Err(Refusal::NoOwner));
    assert_eq!(freeze("   ", true, false), Err(Refusal::EmptyPseud));
}

#[test]
fn the_state_is_the_absent_owner_and_reclaiming_is_never_allowed() {
    assert!(is_orphaned(None));
    assert!(!is_orphaned(Some("acc-1")));
    // Total, not absent: a caller asks the rule, and an adoption feature has to change this visible function.
    assert!(!may_reclaim(true));
    assert!(!may_reclaim(false));
    let _: FrozenLabel = freeze("a", true, false).unwrap();
}
