//! M74-01 — the syndicated trope frontier: per-source windows and a bounded author BFS.
//!
//! Failing first: these tests name the public functions the feature must expose. They are pure beyond
//! the injected fetcher, so they prove the rule rather than the network.

use std::collections::HashMap;
use std::time::Duration;

use darkhold_app::config::{parse_age_window, AuthorSeed, SourceSyncRule, SyndicationConfig};
use darkhold_app::source_sync::{author_id_from_url, discovery_is_walkable};
use darkhold_scrapers::Discovery;

#[test]
fn author_frontier_is_not_skipped_for_having_no_static_listing() {
    assert!(discovery_is_walkable(&Discovery::AuthorFrontier {
        recommended_template: "/patreon/user/{id}/recommended".to_owned(),
    }));
    assert!(discovery_is_walkable(&Discovery::Listing {
        template: "/latest".to_owned(),
    }));
    assert!(!discovery_is_walkable(&Discovery::None));
}

#[test]
fn seed_recommended_url_yields_numeric_author_id() {
    assert_eq!(
        author_id_from_url("https://pawchive.pw/patreon/user/13212571/recommended"),
        "13212571"
    );
    assert_eq!(
        author_id_from_url("https://pawchive.pw/patreon/user/13212571"),
        "13212571"
    );
    assert!(author_id_from_url("https://pawchive.pw/").is_empty());
}

#[test]
fn freshness_windows_parse_and_reject_zero() {
    assert_eq!(
        parse_age_window("6mo").expect("six months"),
        Duration::from_secs(180 * 86_400)
    );
    assert_eq!(
        parse_age_window("1w").expect("one week"),
        Duration::from_secs(7 * 86_400)
    );
    assert_eq!(
        parse_age_window("12h").expect("twelve hours"),
        Duration::from_secs(12 * 3600)
    );
    assert!(parse_age_window("0d").is_err(), "zero is not a window");
    assert!(parse_age_window("7x").is_err(), "unknown units are refused");
}

#[test]
fn default_config_names_pawchive_and_seven_days() {
    let config = SyndicationConfig::default();
    let pawchive = config
        .sources
        .iter()
        .find(|rule| rule.source_id == "pawchive")
        .expect("the shipped Pawchive rule");
    assert_eq!(pawchive.older_than, "6mo");
    assert_eq!(pawchive.body_audience, "trust_at_least:2");
    assert_eq!(
        config
            .older_than_for("squidgeworld")
            .expect("an unnamed source has a default"),
        Duration::from_secs(7 * 86_400)
    );
    let rule = SourceSyncRule {
        source_id: "squidgeworld".to_owned(),
        enabled: true,
        newer_than: Some("2d".to_owned()),
        older_than: "30d".to_owned(),
        body_audience: "trust_at_least:2".to_owned(),
    };
    assert_eq!(
        rule.parse().expect("the rule validates"),
        (
            Some(Duration::from_secs(2 * 86_400)),
            Duration::from_secs(30 * 86_400)
        )
    );
}

#[tokio::test]
async fn bfs_stops_at_depth_three_and_never_revisits_an_author() {
    let seeds = [AuthorSeed {
        source_id: "pawchive".to_owned(),
        author_url: "https://pawchive.pw/patreon/user/26677387".to_owned(),
    }];
    let graph = HashMap::from([
        ("26677387", vec!["5916248", "3728453"]),
        ("5916248", vec!["3728453", "5149963"]),
        ("3728453", vec!["58380582"]),
        ("5149963", vec!["4239971"]),
        ("58380582", vec!["99999999"]),
        ("4239971", vec!["77777777"]),
    ]);
    let nodes = darkhold_app::source_sync::author_frontier(&seeds, 3, 100, |id: String| {
        let graph = &graph;
        async move {
            Ok(graph
                .get(id.as_str())
                .map(|values| values.iter().map(|value| (*value).to_owned()).collect())
                .unwrap_or_default())
        }
    })
    .await;

    assert_eq!(
        nodes
            .iter()
            .map(|node| node.author_id.as_str())
            .collect::<Vec<_>>(),
        vec!["26677387", "5916248", "3728453", "5149963", "58380582", "4239971", "99999999"]
    );
    assert_eq!(
        nodes.iter().map(|node| node.depth).collect::<Vec<_>>(),
        vec![0, 1, 1, 2, 2, 3, 3]
    );
}

#[tokio::test]
async fn bfs_honours_the_author_cap() {
    let seeds = [AuthorSeed {
        source_id: "pawchive".to_owned(),
        author_url: "https://pawchive.pw/patreon/user/1".to_owned(),
    }];
    let graph = HashMap::from([("1", vec!["2", "3", "4", "5"])]);
    let nodes = darkhold_app::source_sync::author_frontier(&seeds, 3, 3, |id: String| {
        let graph = &graph;
        async move {
            Ok(graph
                .get(id.as_str())
                .map(|values| values.iter().map(|value| (*value).to_owned()).collect())
                .unwrap_or_default())
        }
    })
    .await;
    assert_eq!(nodes.len(), 3, "the cap includes the seed");
}
