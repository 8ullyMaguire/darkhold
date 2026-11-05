//! M69-02 (first slice) — the ranking rules, each with the mistake it prevents.

use std::collections::BTreeMap;

use darkhold_domain::peering::{rank, MAX_SUGGESTIONS, MIN_SIMILARITY};
use darkhold_domain::taste_fingerprint::build;

fn print(instance: &str, pairs: &[(&str, f64)]) -> darkhold_domain::taste_fingerprint::Fingerprint {
    let vector: BTreeMap<String, f64> = pairs.iter().map(|(t, w)| ((*t).to_owned(), *w)).collect();
    build(instance, "2026-11-05T00:00:00Z", &vector)
}

#[test]
fn an_instance_is_never_suggested_to_itself() {
    let own = print("https://me.example", &[("angst", 3.0), ("slow burn", 1.0)]);
    // The same profile, fetched back from itself — identical, so it would score 1.0 and sit at the top.
    let twin = print("https://me.example", &[("angst", 3.0), ("slow burn", 1.0)]);
    assert!(rank(&own, &[twin], 10).is_empty());
}

#[test]
fn an_unreadable_profile_is_not_a_zero() {
    let own = print("https://me.example", &[("angst", 1.0)]);
    let blank = print("https://blank.example", &[]);
    let other = print("https://other.example", &[("comedy", 1.0)]);
    let ranked = rank(&own, &[blank, other], 10);
    // `blank` cannot be compared at all, and `other` is comparable but disjoint (0.0, below the floor).
    assert!(ranked.is_empty(), "{ranked:?}");
}

#[test]
fn the_floor_and_the_tie_break_hold() {
    let own = print("https://me.example", &[("a", 1.0), ("b", 1.0), ("c", 1.0)]);
    let close = print(
        "https://close.example",
        &[("a", 1.0), ("b", 1.0), ("c", 1.0)],
    );
    let partial = print(
        "https://partial.example",
        &[("a", 1.0), ("b", 1.0), ("zzz", 1.0)],
    );
    let same_as_partial = print(
        "https://aaa.example",
        &[("a", 1.0), ("b", 1.0), ("zzz", 1.0)],
    );
    let ranked = rank(&own, &[partial, close, same_as_partial], 10);

    assert_eq!(ranked[0].instance_id, "https://close.example");
    assert_eq!(ranked[0].similarity, 1.0);
    // The two partial profiles tie exactly; they come back in id order, not in argument order.
    assert_eq!(ranked[1].instance_id, "https://aaa.example");
    assert_eq!(ranked[2].instance_id, "https://partial.example");
    assert!(ranked[1].similarity >= MIN_SIMILARITY);
}

#[test]
fn the_list_is_capped() {
    let own = print("https://me.example", &[("a", 1.0)]);
    let candidates: Vec<_> = (0..MAX_SUGGESTIONS + 5)
        .map(|index| print(&format!("https://p{index:02}.example"), &[("a", 1.0)]))
        .collect();
    let ranked = rank(&own, &candidates, 100);
    assert_eq!(
        ranked.len(),
        MAX_SUGGESTIONS,
        "the cap is a cap, not a request"
    );
    // And a caller asking for fewer gets fewer.
    assert_eq!(rank(&own, &candidates, 3).len(), 3);
}
