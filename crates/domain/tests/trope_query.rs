//! M45-68 — `trope:<slug>` is a query a reader can type, and it is what it says it is.
//!
//! The expansion happens in the parser, so these tests are about the thing every consumer sees: the
//! AST. A group must become an **`Or` of `tag:` leaves** (a work carrying any member satisfies it), an
//! unknown name must be refused with the list — "no works matched" for a name that was never a group
//! teaches a reader nothing — and the expansion must spend the same term budget as writing the tags
//! out by hand, or `trope:` becomes a way around the guard.

use darkhold_domain::query::{parse_query, QueryAst, QueryField};

fn tags(node: &QueryAst) -> Option<Vec<String>> {
    let QueryAst::Or(parts) = node else {
        return None;
    };
    parts
        .iter()
        .map(|part| match part {
            QueryAst::Fielded(QueryField::Tag, value) => Some(value.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_group_becomes_a_disjunction_of_its_member_tags() {
    let ast = parse_query("trope:dark-progression").expect("a group parses");
    let members = tags(&ast).expect("a group is an Or of tag leaves");
    assert_eq!(
        members,
        vec![
            "dark-protagonist",
            "evil-protagonist",
            "villain-protagonist",
            "morally-grey-protagonist",
            "progression",
        ],
        "the expansion is the table's members, in its order, as tags"
    );
}

#[test]
fn a_group_combines_with_the_rest_of_a_query() {
    // The shape the whole idea exists for: a trope and a fandom, one line.
    let ast = parse_query("trope:system-grind AND tag:naruto").expect("parses");
    let QueryAst::And(parts) = ast else {
        panic!("expected an And");
    };
    assert_eq!(parts.len(), 2, "{parts:?}");
    assert!(tags(&parts[0]).is_some(), "the left half is the group");
    assert_eq!(
        parts[1],
        QueryAst::Fielded(QueryField::Tag, "naruto".to_owned())
    );
}

#[test]
fn an_unknown_group_is_refused_with_the_groups_that_exist() {
    let error = parse_query("trope:enormous").expect_err("refused");
    assert!(
        error.message.contains("not a trope group"),
        "{}",
        error.message
    );
    for slug in darkhold_domain::tropes::slugs() {
        assert!(
            error.message.contains(slug),
            "the refusal lists {slug}: {}",
            error.message
        );
    }
}

#[test]
fn an_expansion_spends_the_same_term_budget_as_writing_the_tags_out() {
    // Three groups at once is 13 member tags: the parser's ceiling is 24 terms at two per tag, so
    // this must be refused — an expansion that bypassed the budget would let a reader ask for a
    // query the engine cannot afford while the same query written by hand was refused.
    let written_out = parse_query(
        "tag:dark-protagonist OR tag:evil-protagonist OR tag:villain-protagonist OR \
         tag:morally-grey-protagonist OR tag:progression OR tag:system OR tag:gamer OR tag:litrpg OR \
         tag:stat-grinding OR tag:multiverse OR tag:dimension-travel OR tag:world-hopping OR \
         tag:dimensional-travel",
    );
    let via_groups =
        parse_query("trope:dark-progression OR trope:system-grind OR trope:multiverse-descent");

    // Absolute, not just symmetric: thirteen tags is thirteen predicates, which is inside a budget
    // of twenty-four. Before the counter stopped charging for connectives this was refused, because
    // the disjunctions were counted as terms too — so this assertion is what pins the definition of
    // "term" rather than merely checking that two spellings agree.
    assert!(
        written_out.is_ok(),
        "thirteen tags is thirteen terms, inside the budget: {written_out:?}"
    );

    match (written_out, via_groups) {
        (Ok(_), Ok(_)) => {}
        (Err(written), Err(groups)) => assert_eq!(
            written.message, groups.message,
            "the same query is refused for the same reason whether it was typed out or named"
        ),
        (written, groups) => {
            panic!("the two spellings disagree: written={written:?} groups={groups:?}")
        }
    }
}
