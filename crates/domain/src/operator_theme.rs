//! M45-69 — the theme an operator states, and which theme actually answers.
//!
//! Two pure rules live here, because both are judgement rather than plumbing:
//!
//! * [`from_seeds`] turns per-seed tag lists into a vector and its top tags. Each tag counts **once per
//!   seed**: a work tagged `dark` twice, or by two of its own tags, is one work saying it once. The
//!   weight is `count / contributing seeds`, so adding a sixth seed does not dilute the first five —
//!   the vector is a description of the shelf, not a share of a pie that grows.
//! * [`precedence`] says which of three themes answers. A season is an event and expires; the shelf is
//!   what the instance is for; bookmarks are what its readers happen to like. Putting bookmarks above
//!   the shelf would mean an operator cannot state anything the audience can contradict by accident.
//!
//! Neither rule touches a database: the store reads rows, this decides what they mean.

use std::collections::BTreeMap;

/// Which theme answered a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// A scheduled window (M45-50) is open.
    Season,
    /// The operator's shelf (M45-69).
    Operator,
    /// Computed from readers' private bookmarks.
    Bookmarks,
    /// Nothing has ever been set: the instance has no stated taste.
    None,
}

impl Source {
    /// The wire name, spelled once so a client and a log agree.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Season => "season",
            Self::Operator => "operator",
            Self::Bookmarks => "bookmarks",
            Self::None => "none",
        }
    }
}

/// Which source answers, given what exists.
///
/// `has_computed` is separate from "the computed vector is empty": an empty vector that was computed is
/// still a computation, and saying `none` would be a different claim.
#[must_use]
pub fn precedence(has_season: bool, has_operator: bool, has_computed: bool) -> Source {
    if has_season {
        Source::Season
    } else if has_operator {
        Source::Operator
    } else if has_computed {
        Source::Bookmarks
    } else {
        Source::None
    }
}

/// A seed's contribution: its label for the record, and the tags it carries.
#[derive(Debug, Clone)]
pub struct SeedTags {
    /// How the seed is named back to the operator (title, or the spec that was pasted).
    pub label: String,
    /// The canonical tag names this seed's work carries.
    pub tags: Vec<String>,
}

/// The vector and the ten heaviest tags, in a stable order.
///
/// Returns `(vector, top_tags)` where both are empty when no seed carried a tag — the caller refuses
/// that case rather than storing an empty claim.
#[must_use]
pub fn from_seeds(seeds: &[SeedTags]) -> (BTreeMap<String, f64>, Vec<(String, f64)>) {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut contributing = 0usize;
    for seed in seeds {
        // A seed's own tag list may repeat a tag; dedupe before counting so one work cannot vote twice.
        let mut seen: Vec<&str> = seed.tags.iter().map(String::as_str).collect();
        seen.sort_unstable();
        seen.dedup();
        if seen.is_empty() {
            continue;
        }
        contributing += 1;
        for tag in seen {
            *counts.entry(tag.to_owned()).or_insert(0) += 1;
        }
    }
    if contributing == 0 {
        return (BTreeMap::new(), Vec::new());
    }
    let denom = contributing as f64;
    let vector: BTreeMap<String, f64> = counts
        .into_iter()
        .map(|(tag, count)| (tag, round4(count as f64 / denom)))
        .collect();
    let mut top: Vec<(String, f64)> = vector.iter().map(|(t, w)| (t.clone(), *w)).collect();
    // Heaviest first; ties by name, so the same shelf always produces the same list — a report that
    // reorders itself between two identical requests is a report nobody can diff.
    top.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    top.truncate(10);
    (vector, top)
}

/// Four decimal places: enough to tell `0.4` from `0.4001`, few enough that a JSON diff stays readable.
fn round4(value: f64) -> f64 {
    (value * 10_000.0).round() / 10_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(label: &str, tags: &[&str]) -> SeedTags {
        SeedTags {
            label: label.to_owned(),
            tags: tags.iter().map(|t| (*t).to_owned()).collect(),
        }
    }

    #[test]
    fn a_tag_counts_once_per_seed_even_when_the_seed_lists_it_twice() {
        let (vector, _) = from_seeds(&[
            seed("one", &["dark", "dark", "progression"]),
            seed("two", &["dark"]),
        ]);
        assert_eq!(vector.get("dark"), Some(&1.0), "two of two seeds carry it");
        assert_eq!(
            vector.get("progression"),
            Some(&0.5),
            "one of two seeds: two works saying a thing is not the same as one work saying it twice"
        );
    }

    #[test]
    fn a_seed_with_no_tags_contributes_nothing_without_erasing_the_others() {
        let (vector, top) = from_seeds(&[seed("tagged", &["dark"]), seed("bare", &[])]);
        assert_eq!(
            vector.get("dark"),
            Some(&1.0),
            "the denominator counts contributing seeds only"
        );
        assert_eq!(top.len(), 1);
    }

    #[test]
    fn no_tags_at_all_is_an_empty_answer_not_a_zero_vector() {
        let (vector, top) = from_seeds(&[seed("bare", &[])]);
        assert!(vector.is_empty() && top.is_empty());
    }

    #[test]
    fn the_top_ten_are_ordered_by_weight_then_name() {
        let mut tags: Vec<String> = (0..12).map(|i| format!("tag{i:02}")).collect();
        let refs: Vec<&str> = tags.iter().map(String::as_str).collect();
        let (_, top) = from_seeds(&[seed("everything", &refs)]);
        assert_eq!(top.len(), 10, "ten, not twelve");
        assert_eq!(
            top[0].0, "tag00",
            "equal weights are name-ordered, so the list is stable"
        );
        tags.clear();
    }

    #[test]
    fn the_shelf_beats_bookmarks_and_a_season_beats_both() {
        assert_eq!(precedence(false, true, true), Source::Operator);
        assert_eq!(precedence(true, true, true), Source::Season);
        assert_eq!(precedence(false, false, true), Source::Bookmarks);
        assert_eq!(precedence(false, false, false), Source::None);
        assert_eq!(Source::Operator.as_str(), "operator");
    }
}
