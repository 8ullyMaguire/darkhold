//! Integration probe for plan Step 2: the pure decision must exist as a public
//! domain module before the store sweep can consume it.

use darkhold_domain::work_lifecycle::{decide, parse_activity_timestamp, LifecycleAction, Reading};

#[test]
fn four_months_is_the_boundary_and_a_day_short_is_not() {
    let now = parse_activity_timestamp("2026-11-24T00:00:00Z").unwrap();
    let stale = Reading {
        completion: "in_progress",
        source_says_complete: false,
        completion_marked_at: None,
        last_activity: Some(parse_activity_timestamp("2026-07-27T00:00:00Z").unwrap()),
    };
    let fresh = Reading {
        completion: "in_progress",
        source_says_complete: false,
        completion_marked_at: None,
        last_activity: Some(parse_activity_timestamp("2026-07-28T00:00:00Z").unwrap()),
    };
    assert_eq!(decide(stale, now, 4), LifecycleAction::Abandon);
    assert_eq!(decide(fresh, now, 4), LifecycleAction::None);
}

#[test]
fn imported_naive_timestamps_are_not_read_as_never_updated() {
    assert_eq!(
        parse_activity_timestamp("2026-08-01T00:00:05"),
        parse_activity_timestamp("2026-08-01T00:00:05Z"),
    );
}
