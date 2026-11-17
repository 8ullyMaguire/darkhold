//! M74-01 — per-source freshness windows are enforced at the candidate boundary.

use darkhold_app::config::{parse_age_window, SourceSyncRule};
use darkhold_app::source_sync::{filter_candidates, is_in_window};
use time::{format_description::well_known::Rfc3339, Duration, OffsetDateTime};

fn rule(newer_than: Option<&str>, older_than: &str) -> SourceSyncRule {
    SourceSyncRule {
        source_id: "pawchive".to_owned(),
        enabled: true,
        newer_than: newer_than.map(str::to_owned),
        older_than: older_than.to_owned(),
        body_audience: "trust_at_least:2".to_owned(),
    }
}

fn at(seconds_ago: i64, now: OffsetDateTime) -> Option<OffsetDateTime> {
    Some(now - Duration::seconds(seconds_ago))
}

#[test]
fn a_window_parses_the_declared_units() {
    assert_eq!(parse_age_window("7d").unwrap().as_secs(), 604_800);
    assert_eq!(parse_age_window("6mo").unwrap().as_secs(), 15_552_000);
    assert!(parse_age_window("0d").is_err());
}

#[test]
fn both_bounds_are_strict_and_undated_candidates_are_skipped() {
    let now = OffsetDateTime::parse(
        "2026-11-17T00:00:00Z",
        &time::format_description::well_known::Rfc3339,
    )
    .unwrap();
    let window = rule(Some("7d"), "1d");

    assert!(
        is_in_window(at(3 * 86_400, now), now, &window),
        "three days old sits inside 1d..7d"
    );
    assert!(
        !is_in_window(at(8 * 86_400, now), now, &window),
        "eight days old is too old"
    );
    assert!(
        !is_in_window(at(12 * 3600, now), now, &window),
        "twelve hours old is too new"
    );
    assert!(
        !is_in_window(None, now, &window),
        "an undated listing cannot be admitted"
    );
    assert!(
        !is_in_window(Some(now + Duration::hours(1)), now, &window),
        "future dates are not old enough"
    );
}

#[test]
fn the_pawchive_default_admits_only_posts_older_than_six_months() {
    let now = OffsetDateTime::parse(
        "2026-11-17T00:00:00Z",
        &time::format_description::well_known::Rfc3339,
    )
    .unwrap();
    let window = rule(None, "6mo");
    assert!(is_in_window(at(181 * 86_400, now), now, &window));
    assert!(!is_in_window(at(179 * 86_400, now), now, &window));
}

#[test]
fn an_invalid_window_never_admits_a_candidate() {
    let now = OffsetDateTime::parse(
        "2026-11-17T00:00:00Z",
        &time::format_description::well_known::Rfc3339,
    )
    .unwrap();
    assert!(!is_in_window(
        at(10 * 86_400, now),
        now,
        &rule(None, "bogus")
    ));
}

#[test]
fn a_listing_pass_admits_only_the_in_window_candidate() {
    let now = OffsetDateTime::parse("2026-11-17T00:00:00Z", &Rfc3339).unwrap();
    let candidates = vec![
        (
            "https://pawchive.pw/patreon/user/1?story=old-enough".to_owned(),
            at(10 * 86_400, now),
        ),
        (
            "https://pawchive.pw/patreon/user/1?story=too-new".to_owned(),
            at(3 * 86_400, now),
        ),
        (
            "https://pawchive.pw/patreon/user/1?story=undated".to_owned(),
            None,
        ),
    ];
    let (urls, rejected) = filter_candidates(candidates, now, &rule(None, "7d"));
    assert_eq!(
        urls,
        vec!["https://pawchive.pw/patreon/user/1?story=old-enough".to_owned()]
    );
    assert_eq!(rejected, 2);
}
