//! M45-33 — the matching rule and the state machine, before any store.

use darkhold_domain::specialist::{rank, summary, transition, Action, Candidate, Skill, State};

fn candidate(id: &str, fandoms: &[&str], live: i64) -> Candidate {
    Candidate {
        pseud_id: id.to_owned(),
        fandoms: fandoms.iter().map(|f| (*f).to_owned()).collect(),
        live_engagements: live,
        completed: 0,
    }
}

#[test]
fn a_named_fandom_outranks_an_open_hand_and_both_outrank_other_fandoms() {
    let ranked = rank(
        "naruto",
        vec![
            candidate("any", &[], 0),
            candidate("named", &["Naruto"], 3),
            candidate("elsewhere", &["bleach"], 0),
        ],
    );
    let order: Vec<&str> = ranked
        .iter()
        .map(|row| row.candidate.pseud_id.as_str())
        .collect();
    assert_eq!(
        order,
        vec!["named", "any"],
        "elsewhere is not an offer for this work: {order:?}"
    );
    assert_eq!(ranked[0].fit, 2);
    assert_eq!(ranked[1].fit, 1);
}

#[test]
fn equal_fit_is_broken_by_load_and_then_by_id_so_the_order_is_total() {
    let ranked = rank(
        "naruto",
        vec![
            candidate("busy", &["naruto"], 4),
            candidate("free", &["naruto"], 0),
            candidate("free-2", &["naruto"], 0),
        ],
    );
    let order: Vec<&str> = ranked
        .iter()
        .map(|row| row.candidate.pseud_id.as_str())
        .collect();
    assert_eq!(order, vec!["free", "free-2", "busy"], "{order:?}");
}

#[test]
fn a_request_naming_no_fandom_ranks_everyone_and_the_summary_says_so() {
    let ranked = rank(
        "",
        vec![candidate("a", &["naruto"], 1), candidate("b", &[], 0)],
    );
    assert_eq!(ranked.len(), 2, "no fandom named means anyone may take it");
    assert_eq!(
        ranked[0].candidate.pseud_id, "b",
        "load decides when fit is equal"
    );
    assert!(
        summary("", &ranked).contains("open to any fandom"),
        "{}",
        summary("", &ranked)
    );
    assert!(
        summary("naruto", &[]).contains("stays open"),
        "{}",
        summary("naruto", &[])
    );
}

#[test]
fn the_state_machine_has_no_path_into_matched_except_accept() {
    assert_eq!(transition(State::Open, Action::Accept), Ok(State::Matched));
    assert_eq!(
        transition(State::Matched, Action::Complete),
        Ok(State::Completed)
    );
    assert_eq!(
        transition(State::Open, Action::Withdraw),
        Ok(State::Withdrawn)
    );
    assert_eq!(
        transition(State::Matched, Action::Withdraw),
        Ok(State::Withdrawn),
        "availability changes, and a matched request must be releasable"
    );
    // Finishing twice, or completing something nobody took, is refused by name.
    assert!(transition(State::Completed, Action::Complete).is_err());
    assert!(transition(State::Open, Action::Complete).is_err());
    assert!(transition(State::Withdrawn, Action::Accept).is_err());
}

#[test]
fn an_unknown_skill_is_refused_rather_than_invented() {
    assert_eq!(Skill::parse("beta"), Some(Skill::Beta));
    assert_eq!(Skill::parse(" BRITPICK "), Some(Skill::Britpick));
    assert_eq!(Skill::parse("betamax"), None);
    assert_eq!(Skill::all().len(), 4);
}
