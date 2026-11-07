//! M69-05 — who may mirror this instance's work, and what a reader is told about it.
//!
//! # The two halves, and which one this is
//!
//! M45-39 is the paperwork of instance-death protection: mutual backups, a dead-man's switch, an operator agreeing in
//! advance. This module is the half a **reader** sees, and it is deliberately narrow: it decides which peers are close enough
//! in taste to be worth handing a copy to, and it says what the work page must print when one of them holds it.
//!
//! The attribution is not decoration. A mirror without a name on it reads as plagiarism — the reader sees the same story on
//! two hosts, and the second one looks like a copy of the first whoever actually wrote it. The author's page is the second
//! place a reader looks, which is why the sentence names the peer and states plainly that this copy lives elsewhere.
//!
//! # Why the floor is higher than a suggestion
//!
//! `peering::MIN_SIMILARITY` (0.25) answers "is this instance interesting enough to tell a reader about". Mirroring is a
//! different question: it is a durable commitment of storage and of the author's work to somebody else's keeping. A peer one
//! tag away is a suggestion; a mirror wants [`MIN_MIRROR_SIMILARITY`], and the operator can raise it but not lower it —
//! the floor is a policy the instance may tighten, never a knob a hurried operator can open to zero.
//!
//! # Why a veto beats a high score here too
//!
//! A peer whose declared hardcoded layer collides with this instance's must not be handed a copy, however similar its taste:
//! the copy would be served under rules this instance does not keep. `peering::assess` already reports that collision and
//! `rank` already drops vetoed peers; mirroring reuses both rather than re-deriving eligibility, because two answers to "may
//! this peer be trusted with a copy" is one answer too many.

use crate::peering::{assess, Verdict};
use crate::taste_fingerprint::Fingerprint;

/// The lowest similarity at which a peer is eligible to hold a mirror.
///
/// Twice `peering::MIN_SIMILARITY`, and for a reason a reader can be told: a mirror is a copy of a specific work kept for
/// that work's sake, so the peer should be closer than "a reader might like it there".
pub const MIN_MIRROR_SIMILARITY: f64 = 0.5;

/// A peer this instance would hand a work to.
#[derive(Debug, Clone, PartialEq)]
pub struct Eligible {
    /// The peer, by the id its fingerprint declares.
    pub instance_id: String,
    /// How close its taste is to this instance's, 0..=1.
    pub similarity: f64,
    /// Whether `similarity` is a measurement (false when either document has no tags, in which case the peer is not here at
    /// all rather than here with a zero).
    pub measured: bool,
}

/// Why a particular peer cannot hold a mirror.
#[derive(Debug, Clone, PartialEq)]
pub enum Ineligible {
    /// No fingerprint from it has been recorded.
    Unknown,
    /// Its taste is measured, and too far away. Carries the measurement so the operator can see how far.
    TooDistant {
        /// The similarity that failed the floor.
        similarity: f64,
    },
    /// It enforces rules this instance does not, or vice versa.
    Collision {
        /// The first rule in the symmetric difference, sorted, so the answer is stable.
        rule: String,
        /// How many rules diverged in total.
        rules: usize,
        /// Which side enforces the named rule: `peer` means they refuse something this instance serves.
        enforced_by: String,
    },
    /// Either document has no tags, so there is nothing to compare.
    Unmeasured,
}

impl Ineligible {
    /// A sentence an operator can read.
    #[must_use]
    pub fn reason(&self) -> String {
        match self {
            Self::Unknown => "no fingerprint from that instance has been recorded".to_owned(),
            Self::TooDistant { similarity } => format!(
                "taste similarity {similarity:.3} is below the mirror floor of {MIN_MIRROR_SIMILARITY:.2}"
            ),
            Self::Collision {
                rule,
                rules,
                enforced_by,
            } => format!(
                "this instance and that peer enforce different rules ({rule}, enforced by {enforced_by}; {rules} in total), \
                 so a copy would be served under rules this instance does not keep"
            ),
            Self::Unmeasured => "a fingerprint is published but carries no tags, so there is nothing to compare".to_owned(),
        }
    }
}

/// Decide whether one peer may hold a mirror of this instance's work.
pub fn eligible(own: &Fingerprint, peer: &Fingerprint) -> Result<Eligible, Ineligible> {
    let instance_id = peer.instance_id.clone();
    match assess(own, peer) {
        Verdict::Collision {
            rule,
            rules,
            enforced_by,
            ..
        } => Err(Ineligible::Collision {
            rule,
            rules,
            enforced_by: enforced_by.to_owned(),
        }),
        Verdict::Unmeasured { .. } => Err(Ineligible::Unmeasured),
        // A peer below the suggestion floor is reported as a near miss only when it is *measured*; `NotEnough` means the
        // measurement exists but did not reach the bar, which is exactly `TooDistant`.
        Verdict::NotEnough { similarity, .. } => Err(Ineligible::TooDistant { similarity }),
        Verdict::Suggested { similarity, .. } => {
            if similarity >= MIN_MIRROR_SIMILARITY {
                Ok(Eligible {
                    instance_id,
                    similarity,
                    measured: true,
                })
            } else {
                Err(Ineligible::TooDistant { similarity })
            }
        }
    }
}

/// Every peer eligible to hold a mirror, closest first, ties broken by instance id.
///
/// Ties are broken by id rather than by the order the store returned, so two runs over the same fingerprints produce the
/// same list: an operator's screen that reorders itself is an operator who stops trusting it.
#[must_use]
pub fn eligible_peers(own: &Fingerprint, peers: &[Fingerprint], floor: f64) -> Vec<Eligible> {
    let floor = floor.max(MIN_MIRROR_SIMILARITY);
    let mut out: Vec<Eligible> = peers
        .iter()
        .filter_map(|peer| match eligible(own, peer) {
            Ok(found) if found.similarity >= floor => Some(found),
            _ => None,
        })
        .collect();
    out.sort_by(|left, right| {
        right
            .similarity
            .partial_cmp(&left.similarity)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.instance_id.cmp(&right.instance_id))
    });
    out
}

/// What a work page prints when this work also lives somewhere else.
#[derive(Debug, Clone, PartialEq)]
pub struct Attribution {
    /// The instance holding the copy.
    pub peer: String,
    /// Where a reader can find the original there. Empty when the peer gave no address.
    pub peer_url: String,
    /// When the copy was made.
    pub mirrored_at: String,
}

impl Attribution {
    /// The sentence a work page shows.
    ///
    /// Written as a statement about the copy, in the second person of the page: "this copy", not "a copy". A reader who has
    /// landed on the mirror should be able to tell in one line that they are on one.
    #[must_use]
    pub fn sentence(&self) -> String {
        if self.peer_url.is_empty() {
            format!(
                "This copy is preserved by {peer}, which holds a mirror of it. (Since {since}.)",
                peer = self.peer,
                since = self.mirrored_at
            )
        } else {
            format!(
                "This copy is preserved by {peer} at {url}, which holds a mirror of it. (Since {since}.)",
                peer = self.peer,
                url = self.peer_url,
                since = self.mirrored_at
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taste_fingerprint::build;

    fn fingerprint(id: &str, tags: &[(&str, f64)]) -> Fingerprint {
        let mut vector = std::collections::BTreeMap::new();
        for (tag, weight) in tags {
            vector.insert((*tag).to_owned(), *weight);
        }
        build(id, "2026-11-07T00:00:00Z", &vector)
    }

    fn own() -> Fingerprint {
        fingerprint("https://a.example", &[("angst", 0.6), ("slowburn", 0.4)])
    }

    #[test]
    fn a_close_peer_is_eligible_and_a_distant_one_is_not() {
        let close = fingerprint("https://close.example", &[("angst", 0.5), ("slowburn", 0.5)]);
        let far = fingerprint("https://far.example", &[("crack", 1.0)]);
        let who = eligible(&own(), &close).expect("close is eligible");
        assert!(who.similarity > 0.5, "{who:?}");
        match eligible(&own(), &far) {
            Err(Ineligible::TooDistant { similarity }) => assert!(similarity < MIN_MIRROR_SIMILARITY, "{similarity}"),
            other => panic!("a distant peer must be too distant, got {other:?}"),
        }
    }

    #[test]
    fn a_peer_enforcing_another_layer_is_refused_however_similar() {
        let mut twin = fingerprint("https://twin.example", &[("angst", 0.6), ("slowburn", 0.4)]);
        twin.enforced.insert("no-archival".to_owned());
        match eligible(&own(), &twin) {
            Err(Ineligible::Collision {
                rule,
                rules,
                enforced_by,
            }) => {
                assert_eq!(rule, "no-archival");
                assert_eq!(rules, 1);
                assert_eq!(enforced_by, "peer");
            }
            other => panic!("a colliding peer must be refused, got {other:?}"),
        }
    }

    #[test]
    fn a_peer_above_the_suggestion_floor_but_below_the_mirror_floor_is_refused_with_its_number() {
        // The gap between the two floors is the design: 0.372 is a peer worth *telling a reader about* (peering's floor is
        // 0.25) and not a peer to hand a copy of somebody's work to. Without this test the mirror floor is a number no
        // behaviour depends on -- and removing the check entirely would leave the suite green.
        let neighbour = fingerprint("https://near-ish.example", &[("angst", 1.0), ("crack", 2.0)]);
        match eligible(&own(), &neighbour) {
            Err(Ineligible::TooDistant { similarity }) => {
                assert!(
                    similarity > crate::peering::MIN_SIMILARITY,
                    "the case only means something above the suggestion floor, got {similarity}"
                );
                assert!(
                    similarity < MIN_MIRROR_SIMILARITY,
                    "and below the mirror floor, got {similarity}"
                );
            }
            other => panic!("expected TooDistant, got {other:?}"),
        }
    }

    #[test]
    fn an_unmeasured_peer_is_not_a_zero_and_is_not_eligible() {
        let empty = fingerprint("https://empty.example", &[]);
        assert_eq!(eligible(&own(), &empty), Err(Ineligible::Unmeasured));
    }

    #[test]
    fn the_floor_is_a_policy_that_can_be_raised_never_lowered() {
        let close = fingerprint("https://close.example", &[("angst", 0.5), ("slowburn", 0.5)]);
        let far = fingerprint("https://far.example", &[("angst", 0.3), ("slowburn", 0.1), ("crack", 0.6)]);
        // A floor of zero does not mean "everybody": `MIN_MIRROR_SIMILARITY` still applies.
        let under = eligible_peers(&own(), &[close.clone(), far.clone()], 0.0);
        assert!(under.iter().all(|peer| peer.similarity >= MIN_MIRROR_SIMILARITY));
        assert!(!under.iter().any(|peer| peer.instance_id == far.instance_id));
        // Raising it does exclude a peer that would otherwise pass.
        let raised = eligible_peers(&own(), std::slice::from_ref(&close), 0.99);
        assert!(raised.is_empty(), "{raised:?}");
    }

    #[test]
    fn the_list_is_order_stable_for_equal_similarities() {
        let left = fingerprint("https://b.example", &[("angst", 0.5), ("slowburn", 0.5)]);
        let right = fingerprint("https://a.example", &[("angst", 0.5), ("slowburn", 0.5)]);
        let first = eligible_peers(&own(), &[left.clone(), right.clone()], MIN_MIRROR_SIMILARITY);
        let second = eligible_peers(&own(), &[right, left], MIN_MIRROR_SIMILARITY);
        assert_eq!(first, second, "the same fingerprints must give the same order");
        assert_eq!(first[0].instance_id, "https://a.example", "ties break by id");
    }

    #[test]
    fn the_attribution_names_the_peer_and_says_it_is_a_mirror() {
        let with_url = Attribution {
            peer: "https://peer.example".to_owned(),
            peer_url: "https://peer.example/works/storm".to_owned(),
            mirrored_at: "2026-11-07".to_owned(),
        };
        let sentence = with_url.sentence();
        assert!(sentence.contains("https://peer.example/works/storm"), "{sentence}");
        assert!(sentence.contains("mirror"), "{sentence}");
        assert!(sentence.contains("2026-11-07"), "{sentence}");

        let without = Attribution {
            peer_url: String::new(),
            ..with_url
        };
        assert!(!without.sentence().contains(" at ,"), "no dangling address: {}", without.sentence());
    }
}
