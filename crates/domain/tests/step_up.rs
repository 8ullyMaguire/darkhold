//! M70-08 — three states, not two.

use darkhold_domain::step_up::{check, level, Requirement, Verdict};

#[test]
fn both_known_acr_families_rank_the_same() {
    assert_eq!(level("1"), Some(1));
    assert_eq!(level("2"), Some(2));
    assert_eq!(level("urn:mace:incommon:iap:aal2"), Some(2));
    assert_eq!(level("URN:example:AAL3"), Some(3));
    assert_eq!(
        level("  2 "),
        Some(2),
        "an issuer may pad; the value is the value"
    );
}

#[test]
fn an_unrankable_acr_is_unknown_rather_than_zero() {
    // Zero would mean "worse than level 1" and would be a claim about a session nobody made a claim about.
    for unknown in ["", "   ", "silver", "urn:example:loa:high", "9"] {
        assert_eq!(level(unknown), None, "{unknown}");
    }
}

#[test]
fn a_second_factor_requirement_is_met_by_two_and_above_only() {
    let requirement = Requirement::SecondFactor;
    assert_eq!(check(Some("2"), requirement), Verdict::Satisfied);
    assert_eq!(check(Some("aal3"), requirement), Verdict::Satisfied);
    assert_eq!(
        check(Some("1"), requirement),
        Verdict::StepUp { have: 1, need: 2 }
    );
    assert_eq!(
        check(Some("aal1"), requirement),
        Verdict::StepUp { have: 1, need: 2 }
    );
}

#[test]
fn an_ordinary_session_requirement_is_met_by_anything_ranked() {
    let requirement = Requirement::Session;
    assert_eq!(check(Some("1"), requirement), Verdict::Satisfied);
    assert_eq!(check(Some("3"), requirement), Verdict::Satisfied);
    // Unknown still is not satisfied: "no assurance asserted" is not "level 1".
    assert_eq!(check(None, requirement), Verdict::Unknown { need: 1 });
}

#[test]
fn a_password_session_is_unknown_and_that_is_the_whole_point() {
    // NULL acr — the ordinary case on a site without single sign-on. Neither low nor high.
    assert_eq!(
        check(None, Requirement::SecondFactor),
        Verdict::Unknown { need: 2 },
        "the guard, not this function, decides whether unknown is admissible"
    );
}
