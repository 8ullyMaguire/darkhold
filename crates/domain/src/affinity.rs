//! M69-09 — reader affinity: how close a reader's taste is to the instance's, as one stored number.
//!
//! # The distinction this module exists to keep
//!
//! Darkhold already stores an operator affinity **per work** (M11-03: how much the operator likes *this* work) and a
//! resonance score. Neither answers "how close is this reader to the operator", and neither can be summed into it: a
//! reader who likes ten works the operator likes and ten the operator hates has the same per-work total as one who
//! likes ten mediocre works, and they are not the same reader. So the unit here is the **reader**, the number is 0..1,
//! and it is a *reading of evidence*, not a per-work multiplier.
//!
//! # Adapter over the estate kernel
//!
//! Since M73-02 the arithmetic lives in `polaris-affinity`. This file keeps darkhold's three weights, its vocabulary
//! and its public `Components`/`Score` types; it only declares which evidence exists and projects the crate's answer.
//! There is no second clamp, division or set arithmetic here on purpose — the estate's tests guard against that.

use polaris_affinity::{
    evidence::{weighted_mean, Component},
    similarity::{agreement_share, cosine_counts, jaccard as set_jaccard},
};

/// The weight of each signal. They sum to 1.0, which is what makes an all-present score a weighted mean.
pub const FAVOURITE_WEIGHT: f64 = 0.5;
/// See [`FAVOURITE_WEIGHT`].
pub const TAG_WEIGHT: f64 = 0.3;
/// See [`FAVOURITE_WEIGHT`].
pub const ARENA_WEIGHT: f64 = 0.2;

/// The evidence for one reader, each component `None` when there is nothing to measure.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Components {
    /// Share of the reader's collected works that sit in the instance's valued set, 0..1.
    pub favourite_overlap: Option<f64>,
    /// Cosine similarity of the reader's tag distribution with the instance's, 0..1.
    pub tag_similarity: Option<f64>,
    /// Share of jointly-judged arena pairs where both picked the same winner, 0..1.
    pub arena_agreement: Option<f64>,
}

impl Components {
    /// The components that carry evidence, with their weights.
    fn present(&self) -> Vec<Component> {
        let mut present = Vec::new();
        if let Some(value) = self.favourite_overlap {
            present.push(Component::new(FAVOURITE_WEIGHT, Some(value)));
        }
        if let Some(value) = self.tag_similarity {
            present.push(Component::new(TAG_WEIGHT, Some(value)));
        }
        if let Some(value) = self.arena_agreement {
            present.push(Component::new(ARENA_WEIGHT, Some(value)));
        }
        present
    }

    /// How many signals carry evidence. Zero means this reader cannot be scored yet.
    #[must_use]
    pub fn evidence_count(&self) -> usize {
        self.present().len()
    }
}

/// A computed affinity, in basis points so it is comparable and storable exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Score {
    /// 0..=10_000.
    pub basis_points: u16,
    /// How much relative weight the present signals carried, in basis points — the denominator a reader's score was
    /// measured against. Published with the score because 0.62 from one signal and 0.62 from three are different
    /// claims, and a reader is entitled to know which one they are looking at.
    pub evidence_bp: u16,
    /// How many signals contributed.
    pub signals: u8,
}

/// Compute the score, or `None` when no signal has evidence.
///
/// The denominator is the sum of the *present* weights, not 1.0 — that is the reweighting. With all three present the
/// score is a plain weighted mean; with one present it is that signal's value, which is the honest reading of a single
/// piece of evidence.
#[must_use]
pub fn score(components: &Components) -> Option<Score> {
    let measured = weighted_mean(&components.present())?;
    #[allow(clippy::cast_possible_truncation)]
    let signals = measured.signals as u8;
    Some(Score {
        basis_points: measured.basis_points,
        evidence_bp: measured.evidence_bp,
        signals,
    })
}

/// Jaccard overlap of two id sets, `None` when either side has no ids to compare.
///
/// The reader's side decides emptiness: an instance with no valued works is a configuration problem, not a score of
/// zero for everybody, and returning `Some(0.0)` there would quietly grade the whole readership on an empty shelf.
#[must_use]
pub fn jaccard(reader: &[String], instance: &[String]) -> Option<f64> {
    set_jaccard(reader, instance)
}

/// Cosine similarity of two tag-count vectors keyed by tag.
///
/// Cosine rather than Jaccard because tags are counts: a reader who tags a hundred works `angst` and the instance
/// which values three `angst` works are not "0% similar", they are a reader whose strongest interest the instance
/// shares. `None` when either side has no tags — the same rule as [`jaccard`], on both sides this time, because a
/// cosine needs two vectors.
#[must_use]
pub fn cosine(
    reader: &std::collections::BTreeMap<String, f64>,
    instance: &std::collections::BTreeMap<String, f64>,
) -> Option<f64> {
    cosine_counts(reader, instance)
}

/// Agreement over jointly-judged arena pairs.
///
/// `None` when there is no shared pair — that is the state of every reader who has never opened the arena, and it is
/// the reason the component is optional rather than zero.
#[must_use]
pub fn arena_agreement(shared_pairs: usize, agreed_pairs: usize) -> Option<f64> {
    agreement_share(shared_pairs, agreed_pairs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn the_adapter_does_not_carry_a_second_arithmetic() {
        let source = include_str!("affinity.rs");
        let local_clamp = ["fn clamp", "01"].concat();
        let set_intersection = ["intersection", "("].concat();
        let local_denominator = ["total", "_weight"].concat();
        assert!(
            !source.contains(&local_clamp),
            "the clamp belongs to polaris-affinity"
        );
        assert!(
            !source.contains(&set_intersection),
            "set arithmetic belongs to polaris-affinity"
        );
        assert!(
            !source.contains(&local_denominator),
            "reweighting belongs to polaris-affinity"
        );
    }

    #[test]
    fn all_three_present_is_their_weighted_mean() {
        let score = score(&Components {
            favourite_overlap: Some(1.0),
            tag_similarity: Some(0.5),
            arena_agreement: Some(0.0),
        })
        .expect("scored");
        // 0.5*1.0 + 0.3*0.5 + 0.2*0.0 = 0.65
        assert_eq!(score.basis_points, 6_500);
        assert_eq!(score.signals, 3);
        assert_eq!(score.evidence_bp, 10_000);
    }

    #[test]
    fn an_absent_signal_is_removed_from_both_halves_not_counted_as_zero() {
        let one = score(&Components {
            favourite_overlap: Some(0.8),
            ..Components::default()
        })
        .expect("scored");
        assert_eq!(
            one.basis_points, 8_000,
            "a reader with only favourite evidence is as aligned as that evidence says, not 40% of it"
        );
        assert_eq!(
            one.evidence_bp, 5_000,
            "the denominator travels with the score"
        );
        assert_eq!(one.signals, 1);

        let two = score(&Components {
            favourite_overlap: Some(0.8),
            arena_agreement: Some(0.4),
            ..Components::default()
        })
        .expect("scored");
        // (0.5*0.8 + 0.2*0.4) / 0.7 = 0.6857
        assert_eq!(two.basis_points, 6_857);
        assert_eq!(two.signals, 2);
    }

    #[test]
    fn no_evidence_is_no_score_rather_than_zero() {
        assert_eq!(score(&Components::default()), None);
        assert_eq!(Components::default().evidence_count(), 0);
        // And a signal that is present-and-zero IS evidence: the reader is measured and disagrees.
        let zero = score(&Components {
            tag_similarity: Some(0.0),
            ..Components::default()
        })
        .expect("a measured zero is a score");
        assert_eq!(zero.basis_points, 0);
        assert_eq!(zero.signals, 1);
    }

    #[test]
    fn components_are_clamped_so_a_ratio_cannot_escape_its_range() {
        let score = score(&Components {
            favourite_overlap: Some(1.000_000_2),
            tag_similarity: Some(-0.3),
            arena_agreement: Some(f64::NAN),
        })
        .expect("scored");
        assert!(score.basis_points <= 10_000);
        // NaN is treated as zero rather than poisoning the mean into NaN basis points.
        assert_eq!(score.signals, 3);
    }

    #[test]
    fn jaccard_reads_the_readers_side_for_emptiness() {
        let reader = vec!["a".to_owned(), "b".to_owned()];
        let instance = vec!["b".to_owned(), "c".to_owned()];
        let overlap = jaccard(&reader, &instance).expect("overlap");
        assert!((overlap - 1.0 / 3.0).abs() < 1e-9, "{overlap}");

        assert_eq!(
            jaccard(&[], &instance),
            None,
            "an empty reader has no overlap to measure"
        );
        assert_eq!(
            jaccard(&reader, &[]),
            None,
            "an instance that values nothing has not measured anything, so it is no score rather than zero"
        );
    }

    #[test]
    fn cosine_rewards_a_shared_strong_interest() {
        let reader = BTreeMap::from([("angst".to_owned(), 100.0), ("fluff".to_owned(), 1.0)]);
        let instance = BTreeMap::from([("angst".to_owned(), 3.0), ("smut".to_owned(), 2.0)]);
        let similarity = cosine(&reader, &instance).expect("similarity");
        // 300 / (sqrt(10001) * sqrt(13)) ≈ 0.8319
        assert!(similarity > 0.83 && similarity < 0.84, "{similarity}");
        assert_eq!(cosine(&BTreeMap::new(), &instance), None);
        assert_eq!(cosine(&reader, &BTreeMap::new()), None);
    }

    #[test]
    fn arena_agreement_counts_only_shared_pairs() {
        assert_eq!(arena_agreement(0, 0), None);
        assert_eq!(arena_agreement(4, 3), Some(0.75));
        assert_eq!(
            arena_agreement(4, 0),
            Some(0.0),
            "four shared pairs and no agreement is a zero"
        );
    }
}
