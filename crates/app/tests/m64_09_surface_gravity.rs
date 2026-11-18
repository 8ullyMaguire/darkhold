//! M64-09 — the same bounded pull on the two reader rails that still ignored it.

use std::collections::HashMap;

use darkhold_app::surface_gravity::reorder;
use darkhold_db::reader_surface::SurfaceWork;

fn work(id: &str, title: &str) -> SurfaceWork {
    SurfaceWork {
        id: id.to_owned(),
        title: title.to_owned(),
        summary: String::new(),
        completion: "complete".to_owned(),
        published_at: None,
        word_count: 100,
        recent_bookmarks: None,
        similarity: None,
    }
}

#[test]
fn gravity_reorders_without_filtering_and_without_editing_scores() {
    let first = "01955c00-0000-4000-8000-000000000001";
    let second = "01955c00-0000-4000-8000-000000000002";
    let mut works = vec![work(first, "cold"), work(second, "warm")];
    works[1].similarity = Some(0.42);
    let mut signals = HashMap::new();
    signals.insert(first.to_owned(), 0.0);
    signals.insert(second.to_owned(), 1.0);

    let got = reorder(works, signals, 1.0, "taste_weighted");

    assert_eq!(got.len(), 2, "gravity must permute, never filter");
    assert_eq!(got[0].id, second, "the warm work should be pulled up");
    assert_eq!(
        got[0].similarity,
        Some(0.42),
        "the engine's score is untouched"
    );
    assert_eq!(got[1].id, first);
}

#[test]
fn a_zero_strength_is_the_identity_permutation() {
    let first = "01955c00-0000-4000-8000-000000000001";
    let second = "01955c00-0000-4000-8000-000000000002";
    let works = vec![work(first, "cold"), work(second, "warm")];
    let mut signals = HashMap::new();
    signals.insert(second.to_owned(), 1.0);

    let got = reorder(works, signals, 0.0, "taste_weighted");
    assert_eq!(got[0].id, first);
    assert_eq!(got[1].id, second);
}
