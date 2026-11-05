//! M69-01 — the fingerprint shape: comparable between instances, safe to publish.

use std::collections::BTreeMap;

use darkhold_domain::taste_fingerprint::{build, similarity, MAX_TAGS, VERSION};

fn vector(pairs: &[(&str, f64)]) -> BTreeMap<String, f64> {
    pairs
        .iter()
        .map(|(tag, weight)| ((*tag).to_owned(), *weight))
        .collect()
}

#[test]
fn weights_are_normalised_and_the_order_is_stable() {
    let print = build(
        "https://darkhold.example/",
        "2026-11-05T10:00:00Z",
        &vector(&[("slow burn", 3.0), ("Angst", 1.0), ("hurt/comfort", 4.0)]),
    );
    assert_eq!(print.version, VERSION);
    assert_eq!(
        print.instance_id, "https://darkhold.example",
        "the trailing slash is normalised"
    );
    // 4/8, 3/8, 1/8 — normalised so a small instance and a large one can be compared.
    assert_eq!(
        print.tags,
        vec![
            darkhold_domain::taste_fingerprint::TagWeight {
                tag: "hurt/comfort".into(),
                weight: 0.5
            },
            darkhold_domain::taste_fingerprint::TagWeight {
                tag: "slow burn".into(),
                weight: 0.375
            },
            darkhold_domain::taste_fingerprint::TagWeight {
                tag: "angst".into(),
                weight: 0.125
            },
        ]
    );
    // Same input, same bytes — a fingerprint that changes between two calls cannot be diffed.
    let again = build(
        "https://darkhold.example",
        "2026-11-05T10:00:00Z",
        &vector(&[("hurt/comfort", 4.0), ("slow burn", 3.0), ("Angst", 1.0)]),
    );
    assert_eq!(print, again);
}

#[test]
fn ties_break_alphabetically_and_negatives_do_not_count_as_interest() {
    let print = build(
        "https://x.example",
        "2026-11-05T10:00:00Z",
        &vector(&[("beta", 2.0), ("alpha", 2.0), ("disliked", -5.0)]),
    );
    assert_eq!(
        print
            .tags
            .iter()
            .map(|row| row.tag.as_str())
            .collect::<Vec<_>>(),
        vec!["alpha", "beta"],
        "a negative weight is not interest, and equal weights sort by name"
    );
    assert_eq!(print.tags[0].weight, 0.5);
}

#[test]
fn a_duplicate_spelling_is_one_tag_and_the_cap_holds() {
    let print = build(
        "https://x.example",
        "2026-11-05T10:00:00Z",
        &vector(&[("Hurt/Comfort", 1.0), ("hurt/comfort", 3.0)]),
    );
    assert_eq!(print.tags.len(), 1);
    assert_eq!(print.tags[0].tag, "hurt/comfort");
    assert_eq!(print.tags[0].weight, 1.0);

    let many: Vec<(String, f64)> = (0..MAX_TAGS + 10)
        .map(|index| (format!("tag-{index:03}"), 1.0))
        .collect();
    let vector: BTreeMap<String, f64> = many.into_iter().collect();
    assert_eq!(
        build("https://x.example", "now", &vector).tags.len(),
        MAX_TAGS
    );
}

#[test]
fn an_empty_or_all_negative_vector_publishes_no_tags_rather_than_nan() {
    let empty = build("https://x.example", "now", &BTreeMap::new());
    assert!(empty.tags.is_empty());
    let negative = build("https://x.example", "now", &vector(&[("nope", -1.0)]));
    assert!(negative.tags.is_empty());
    // And similarity to nothing is undefined, not zero: suggesting peering with a blank document is not a result.
    assert_eq!(similarity(&empty, &negative), None);
}

#[test]
fn similarity_is_cosine_and_identical_profiles_score_one() {
    let profile = vector(&[("a", 3.0), ("b", 1.0)]);
    let left = build("https://a.example", "now", &profile);
    let right = build("https://b.example", "now", &profile);
    assert_eq!(similarity(&left, &right), Some(1.0));

    // Disjoint profiles are 0, and partially overlapping ones sit between.
    let other = build("https://c.example", "now", &vector(&[("c", 1.0)]));
    assert_eq!(similarity(&left, &other), Some(0.0));
    let mixed = build(
        "https://d.example",
        "now",
        &vector(&[("a", 1.0), ("zzz", 1.0)]),
    );
    let overlap = similarity(&left, &mixed).expect("similarity");
    assert!(overlap > 0.0 && overlap < 1.0, "{overlap}");
}
