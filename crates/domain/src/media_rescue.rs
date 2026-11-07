//! M69-06 — the rescue ring: after a peer dies, find the same media at a survivor, by hash.
//!
//! # What "the same media" can honestly mean
//!
//! Two hashes, two different claims:
//!
//! * the **content hash** is identity. If a survivor reports the same bytes, the media is the same media, and the ring may
//!   say so without a human in the loop.
//! * the **perceptual hash** is evidence. dHash survives re-encoding, resizing and mild edits, which is exactly why it can
//!   find a copy, and exactly why it cannot prove one: two different images can land inside the threshold, and the failure
//!   mode is a work's page showing somebody else's picture. So a perceptual match is a **proposal**, never an attachment.
//!
//! # Why a tie is an answer, not a nuisance
//!
//! If two survivors are equally close, picking one is picking arbitrarily — and the cost of picking wrong is an image
//! attached to a work that does not own it, which is worse than an image left missing. [`Outcome::Ambiguous`] carries both
//! names so an operator chooses, and the ring never chooses for them. The same reasoning applies to an exact hash held by
//! two survivors: the *media* is identical, but "which survivor do we credit" is still a decision, and crediting both
//! silently is how two peers end up claiming one recovery.
//!
//! # Dimensions are a second lock, not a score
//!
//! Two hashes that agree while the dimensions disagree by a factor of two are not the same picture — they are a thumbnail
//! beside its original, which is a legitimate pair and the wrong rescue. So a candidate whose dimensions differ from the
//! wanted copy's by more than [`DIMENSION_TOLERANCE`] is excluded outright rather than scored lower. The tolerance is a
//! ratio rather than a pixel count, because a 200×200 and a 204×204 crop are the same asset at different sizes and a
//! 200×200 and a 400×400 are not.

use crate::media_resilience::{hamming_distance, perceptual_match_confidence};

/// How much a candidate's dimensions may differ before it is not the same asset.
pub const DIMENSION_TOLERANCE: f64 = 1.25;

/// The media this instance lost, as a survivor would have to describe it to be matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wanted {
    /// The exact hash of the bytes this instance used to have.
    pub content_hash: String,
    /// The perceptual hash, when one was computed.
    pub perceptual_hash: Option<String>,
    /// Width in pixels, when known.
    pub width: Option<u32>,
    /// Height in pixels, when known.
    pub height: Option<u32>,
}

/// A copy a survivor says it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Copy {
    /// The survivor's instance id.
    pub instance: String,
    /// The exact hash of the bytes it holds.
    pub content_hash: String,
    /// Its perceptual hash, when the survivor computed one.
    pub perceptual_hash: Option<String>,
    /// Width in pixels, when known.
    pub width: Option<u32>,
    /// Height in pixels, when known.
    pub height: Option<u32>,
}

/// What the ring found.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// A survivor holds the identical bytes. `instance` is the one survivor, when there is exactly one.
    Exact {
        /// The survivor holding it.
        instance: String,
    },
    /// One survivor is perceptually close. A proposal, never an attachment.
    Proposal {
        /// The survivor.
        instance: String,
        /// The Hamming distance between the two perceptual hashes.
        distance: u32,
        /// The confidence derived from that distance, so a curator sees the number the search used.
        confidence: f64,
    },
    /// The bytes (or the picture) exist at more than one survivor and the ring will not choose.
    Ambiguous {
        /// The survivors, sorted, so the answer is stable.
        instances: Vec<String>,
        /// Why the choice was left to a human.
        reason: String,
    },
    /// Nothing matched: no survivor reported these bytes, and none is perceptually close enough.
    NotFound,
}

impl Outcome {
    /// The survivor to credit, when the ring named exactly one.
    #[must_use]
    pub fn credited(&self) -> Option<&str> {
        match self {
            Self::Exact { instance } => Some(instance),
            Self::Proposal { instance, .. } => Some(instance),
            Self::Ambiguous { .. } | Self::NotFound => None,
        }
    }
}

fn dimension_ratio(left: Option<u32>, right: Option<u32>) -> Option<f64> {
    match (left, right) {
        (Some(left), Some(right)) if left > 0 && right > 0 => {
            let (low, high) = if left <= right {
                (left, right)
            } else {
                (right, left)
            };
            Some(f64::from(high) / f64::from(low))
        }
        // An unknown dimension is not a mismatch: most survivors will not know, and refusing them all would make the ring
        // unusable in the case it exists for.
        _ => None,
    }
}

fn same_asset(wanted: &Wanted, candidate: &Copy) -> bool {
    let width = dimension_ratio(wanted.width, candidate.width);
    let height = dimension_ratio(wanted.height, candidate.height);
    [width, height]
        .into_iter()
        .flatten()
        .all(|ratio| ratio <= DIMENSION_TOLERANCE)
}

/// Search the survivors for the media.
///
/// `threshold` is the maximum Hamming distance treated as a perceptual match, in the same units as the rest of
/// `media_resilience` (`media_resilience.perceptual_match_threshold`).
#[must_use]
pub fn rescue(wanted: &Wanted, copies: &[Copy], threshold: u32) -> Outcome {
    // Exact first, and over the whole set: identity beats similarity, so a survivor with the identical bytes must not lose
    // to a survivor with a similar picture.
    let mut exact: Vec<&str> = copies
        .iter()
        .filter(|candidate| candidate.content_hash == wanted.content_hash)
        .filter(|candidate| same_asset(wanted, candidate))
        .map(|candidate| candidate.instance.as_str())
        .collect();
    exact.sort_unstable();
    exact.dedup();
    match exact.len() {
        1 => {
            return Outcome::Exact {
                instance: exact[0].to_owned(),
            }
        }
        count if count > 1 => {
            return Outcome::Ambiguous {
                instances: exact.iter().map(|instance| (*instance).to_owned()).collect(),
                reason: format!(
                    "{count} survivors report the identical bytes; which one is credited is a decision, not a lookup"
                ),
            }
        }
        _ => {}
    }

    let Some(wanted_hash) = wanted.perceptual_hash.as_deref() else {
        // Without a perceptual hash there is nothing to compare, and inventing a distance would be inventing the match.
        return Outcome::NotFound;
    };

    let mut measured: Vec<(u32, &str)> = Vec::new();
    for candidate in copies {
        if !same_asset(wanted, candidate) {
            continue;
        }
        let Some(hash) = candidate.perceptual_hash.as_deref() else {
            continue;
        };
        let Some(distance) = hamming_distance(wanted_hash, hash) else {
            // One of the two hashes is not a hash. Skipping is the only honest answer: a distance from a malformed string
            // would be a number with no meaning that a curator would still read as evidence.
            continue;
        };
        if distance <= threshold {
            measured.push((distance, candidate.instance.as_str()));
        }
    }
    measured.sort_unstable();
    measured.dedup();

    let Some(&(best, instance)) = measured.first() else {
        return Outcome::NotFound;
    };
    // A tie at the best distance is left to a human: two survivors equally close means the hashes cannot tell them apart,
    // and picking the first would be picking by alphabetical accident.
    let tied: Vec<String> = measured
        .iter()
        .filter(|(distance, _)| *distance == best)
        .map(|(_, instance)| (*instance).to_owned())
        .collect();
    if tied.len() > 1 {
        return Outcome::Ambiguous {
            instances: tied,
            reason: format!(
                "{count} survivors are equally close at distance {best}; the hash cannot tell them apart",
                count = measured.iter().filter(|(distance, _)| *distance == best).count()
            ),
        };
    }

    Outcome::Proposal {
        instance: instance.to_owned(),
        distance: best,
        confidence: perceptual_match_confidence(best),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wanted(perceptual: Option<&str>) -> Wanted {
        Wanted {
            content_hash: "content-aaaa".to_owned(),
            perceptual_hash: perceptual.map(str::to_owned),
            width: Some(400),
            height: Some(300),
        }
    }

    fn copy(instance: &str, content: &str, perceptual: Option<&str>) -> Copy {
        Copy {
            instance: instance.to_owned(),
            content_hash: content.to_owned(),
            perceptual_hash: perceptual.map(str::to_owned),
            width: Some(400),
            height: Some(300),
        }
    }

    #[test]
    fn identical_bytes_at_one_survivor_is_an_exact_recovery() {
        let copies = [copy("https://alive.example", "content-aaaa", None)];
        assert_eq!(
            rescue(&wanted(None), &copies, 10),
            Outcome::Exact {
                instance: "https://alive.example".to_owned()
            }
        );
    }

    #[test]
    fn identical_bytes_at_two_survivors_is_ambiguous_rather_than_a_coin_flip() {
        let copies = [
            copy("https://b.example", "content-aaaa", None),
            copy("https://a.example", "content-aaaa", None),
        ];
        match rescue(&wanted(None), &copies, 10) {
            Outcome::Ambiguous { instances, reason } => {
                assert_eq!(
                    instances,
                    vec!["https://a.example", "https://b.example"],
                    "sorted, so the answer is stable"
                );
                assert!(reason.contains("2 survivors"), "{reason}");
            }
            other => panic!("expected ambiguity, got {other:?}"),
        }
    }

    #[test]
    fn a_close_picture_is_a_proposal_with_its_distance_and_confidence() {
        let copies = [copy(
            "https://alive.example",
            "content-bbbb",
            Some("ffff0000ffff0000"),
        )];
        match rescue(&wanted(Some("ffff0000ffff0001")), &copies, 10) {
            Outcome::Proposal {
                instance,
                distance,
                confidence,
            } => {
                assert_eq!(instance, "https://alive.example");
                assert_eq!(distance, 1);
                assert!(confidence > 0.9, "{confidence}");
            }
            other => panic!("expected a proposal, got {other:?}"),
        }
    }

    #[test]
    fn exact_beats_similar_even_when_the_similar_one_is_closer_in_the_list() {
        let copies = [
            copy(
                "https://near.example",
                "content-bbbb",
                Some("ffff0000ffff0000"),
            ),
            copy(
                "https://exact.example",
                "content-aaaa",
                Some("0000000000000000"),
            ),
        ];
        assert_eq!(
            rescue(&wanted(Some("ffff0000ffff0000")), &copies, 10),
            Outcome::Exact {
                instance: "https://exact.example".to_owned()
            }
        );
    }

    #[test]
    fn two_equally_close_survivors_are_left_to_a_human() {
        let copies = [
            copy("https://a.example", "content-x", Some("ffff0000ffff0001")),
            copy("https://b.example", "content-y", Some("ffff0000ffff0001")),
        ];
        match rescue(&wanted(Some("ffff0000ffff0000")), &copies, 10) {
            Outcome::Ambiguous { instances, reason } => {
                assert_eq!(instances, vec!["https://a.example", "https://b.example"]);
                assert!(reason.contains("distance 1"), "{reason}");
            }
            other => panic!("expected ambiguity, got {other:?}"),
        }
    }

    #[test]
    fn a_thumbnail_is_not_the_asset() {
        // Same perceptual hash, a quarter of the size: the pair is legitimate and the rescue is wrong.
        let mut thumbnail = copy(
            "https://thumb.example",
            "content-cccc",
            Some("ffff0000ffff0000"),
        );
        thumbnail.width = Some(100);
        thumbnail.height = Some(75);
        assert_eq!(
            rescue(&wanted(Some("ffff0000ffff0000")), &[thumbnail], 10),
            Outcome::NotFound
        );
    }

    #[test]
    fn nothing_similar_is_not_found_and_no_perceptual_hash_means_no_search() {
        let far = copy(
            "https://far.example",
            "content-dd",
            Some("0000000000000000"),
        );
        assert_eq!(
            rescue(&wanted(Some("ffffffffffffffff")), &[far], 4),
            Outcome::NotFound
        );
        // A wanted copy with no perceptual hash cannot be searched perceptually, however close the candidates look.
        let close = copy(
            "https://close.example",
            "content-ee",
            Some("ffffffffffffffff"),
        );
        assert_eq!(rescue(&wanted(None), &[close], 10), Outcome::NotFound);
    }

    #[test]
    fn a_malformed_hash_is_skipped_rather_than_measured() {
        let junk = copy("https://junk.example", "content-ff", Some("not-a-hash"));
        assert_eq!(
            rescue(&wanted(Some("ffff0000ffff0000")), &[junk], 64),
            Outcome::NotFound
        );
    }

    #[test]
    fn an_unknown_dimension_does_not_disqualify_a_survivor() {
        let mut unknown = copy("https://knows-nothing.example", "content-aaaa", None);
        unknown.width = None;
        unknown.height = None;
        assert_eq!(
            rescue(&wanted(None), &[unknown], 10),
            Outcome::Exact {
                instance: "https://knows-nothing.example".to_owned()
            }
        );
    }
}
