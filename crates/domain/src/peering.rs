//! M69-02 — which peers are worth suggesting, decided once, and which can never be.
//!
//! Peering is a suggestion, so the ranking has to be a pure function of what each side published: no session, no
//! hidden state, and no random tie-break that makes the same list come back in a different order twice.
//!
//! Five rules, each of which exists because the alternative is a bad suggestion:
//!
//! * **An unreadable or empty profile is never suggested.** `similarity` returns `None` for it, and "similarity
//!   unknown" is not "similarity zero" — suggesting a peer whose fingerprint we could not compare is a coin flip
//!   dressed as a recommendation.
//! * **The instance is never suggested itself.** Two instances with identical profiles score `1.0`, so without
//!   this the top of every list is the list's owner.
//! * **Below the floor is not a suggestion.** `MIN_SIMILARITY` is a floor on *cosine*, not a rank cut: the tenth
//!   most similar of a bad set is still bad, and a suggestion list that is always full is a list nobody reads.
//! * **Heaviest first, then by instance id.** A tie-break on the id makes the order reproducible, which is what
//!   lets an operator see the same list twice while deciding.
//! * **A hardcoded-layer collision is a veto, not a low score.** Two instances that enforce different rules cannot be
//!   peers however alike their taste is, because the difference is in the one dimension that is not negotiable. The
//!   check runs before any score is reported: a number the operator can see is a number they can act on.
//!
//! The last rule is the one that only darkhold needs. Suki's hardcoded layer is taste (a shared set of filters); this
//! instance's is the set of rules it enforces unconditionally — the things it will not serve at all. An instance that
//! will serve them is not "0.6 similar", it is ineligible, and calling that a low score would let a good cosine
//! outvote it.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::taste_fingerprint::{similarity, Fingerprint};

/// The floor a candidate must clear to be worth showing. Chosen to be visibly above "shares one popular tag":
/// two profiles that agree on one tag out of twenty land far below this.
pub const MIN_SIMILARITY: f64 = 0.25;

/// The most suggestions one call returns. A suggestion surface is an invitation, not a directory.
pub const MAX_SUGGESTIONS: usize = 10;

/// One suggested peer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Suggestion {
    pub instance_id: String,
    pub similarity: f64,
}

/// What comparing two instances decided about the pair.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum Verdict {
    /// Similar enough to be worth showing.
    Suggested {
        instance_id: String,
        similarity: f64,
    },
    /// Similar, but not enough to suggest — the honest middle, kept visible so an operator can see the near misses.
    NotEnough {
        instance_id: String,
        similarity: f64,
    },
    /// The rules one side enforces and the other does not. No score is reported, because none would be acted on.
    Collision {
        instance_id: String,
        /// The first rule in the symmetric difference, sorted, so the verdict is stable.
        rule: String,
        /// How many rules diverged in total.
        rules: usize,
        /// Which side enforces the named rule: `peer` means they refuse something this instance serves.
        enforced_by: &'static str,
    },
    /// Nothing comparable was published, so there is no measurement to make.
    Unmeasured { instance_id: String },
}

impl Verdict {
    /// The instance the verdict is about.
    #[must_use]
    pub fn instance_id(&self) -> &str {
        match self {
            Self::Suggested { instance_id, .. }
            | Self::NotEnough { instance_id, .. }
            | Self::Collision { instance_id, .. }
            | Self::Unmeasured { instance_id } => instance_id,
        }
    }

    /// Whether this verdict recommends the peer.
    #[must_use]
    pub fn is_suggestion(&self) -> bool {
        matches!(self, Self::Suggested { .. })
    }

    /// The score, when there is one to report.
    #[must_use]
    pub fn similarity(&self) -> Option<f64> {
        match self {
            Self::Suggested { similarity, .. } | Self::NotEnough { similarity, .. } => Some(*similarity),
            Self::Collision { .. } | Self::Unmeasured { .. } => None,
        }
    }

    /// One sentence for an operator, naming the number or the rule that decided it.
    #[must_use]
    pub fn reason(&self) -> String {
        match self {
            Self::Suggested {
                instance_id,
                similarity,
            } => format!(
                "{instance_id} scores {similarity:.3}, at or above the {MIN_SIMILARITY:.2} a suggestion needs"
            ),
            Self::NotEnough {
                instance_id,
                similarity,
            } => format!(
                "{instance_id} scores {similarity:.3}, below the {MIN_SIMILARITY:.2} a suggestion needs"
            ),
            Self::Collision {
                instance_id,
                rule,
                rules,
                enforced_by,
            } => format!(
                "{instance_id} enforces a different hardcoded layer: {rule} is enforced by {enforced_by} and not by \
                 the other side ({} rule{} in collision), so no similarity can make it a peer",
                rules,
                if *rules == 1 { "" } else { "s" }
            ),
            Self::Unmeasured { instance_id } => format!(
                "{instance_id} published no comparable taste, so there is nothing to score"
            ),
        }
    }
}

/// The rules one side enforces and the other does not.
///
/// Symmetric by construction: a peer that enforces *more* than this instance is a different question from one that
/// enforces less, and an operator should see both rather than be told "blocked" about a rule they also have.
#[must_use]
pub fn collisions(own: &Fingerprint, peer: &Fingerprint) -> BTreeSet<String> {
    own.enforced
        .symmetric_difference(&peer.enforced)
        .cloned()
        .collect()
}

/// Compare this instance's fingerprint with a peer's and say what an operator should do.
#[must_use]
pub fn assess(own: &Fingerprint, peer: &Fingerprint) -> Verdict {
    // The veto first: a score computed ahead of it would be a number the operator could act on, and that is exactly
    // what must not happen.
    let collided = collisions(own, peer);
    if let Some(rule) = collided.iter().next() {
        return Verdict::Collision {
            instance_id: peer.instance_id.clone(),
            rule: rule.clone(),
            rules: collided.len(),
            enforced_by: if peer.enforced.contains(rule) {
                "peer"
            } else {
                "this instance"
            },
        };
    }

    match similarity(own, peer) {
        None => Verdict::Unmeasured {
            instance_id: peer.instance_id.clone(),
        },
        Some(score) => {
            // `+ 0.0` folds `-0.0` into `0.0`: an orthogonal pair is equally similar in both directions, and a report that
            // prints "-0.000" reads as a bug in the arithmetic rather than as a pair with nothing in common.
            let rounded = (score * 10_000.0).round() / 10_000.0 + 0.0;
            if score >= MIN_SIMILARITY {
                Verdict::Suggested {
                    instance_id: peer.instance_id.clone(),
                    similarity: rounded,
                }
            } else {
                Verdict::NotEnough {
                    instance_id: peer.instance_id.clone(),
                    similarity: rounded,
                }
            }
        }
    }
}

/// Every candidate compared against this instance, suggestions first, then vetoes, then near misses.
///
/// The order is the feature: an operator opening the page wants what they can act on, and a `blocked` row buried under
/// ten near misses is a row nobody reads. A veto ranks above a near miss because a near miss is a maybe and a collision
/// is an answer.
#[must_use]
pub fn suggest(own: &Fingerprint, candidates: &[Fingerprint], limit: usize) -> Vec<Verdict> {
    let mut verdicts: Vec<Verdict> = candidates
        .iter()
        .filter(|candidate| candidate.instance_id != own.instance_id)
        .map(|candidate| assess(own, candidate))
        .collect();
    verdicts.sort_by(|a, b| {
        let rank = |verdict: &Verdict| match verdict {
            Verdict::Suggested { .. } => 0,
            Verdict::Collision { .. } => 1,
            Verdict::NotEnough { .. } => 2,
            Verdict::Unmeasured { .. } => 3,
        };
        rank(a).cmp(&rank(b)).then_with(|| {
            b.similarity()
                .unwrap_or(0.0)
                .total_cmp(&a.similarity().unwrap_or(0.0))
        })
        .then_with(|| a.instance_id().cmp(b.instance_id()))
    });
    verdicts.truncate(limit.min(MAX_SUGGESTIONS));
    verdicts
}

/// Rank candidate fingerprints against this instance's own.
///
/// `candidates` are fingerprints fetched from other instances; `own` is this instance's published one. The own
/// fingerprint's `instance_id` is excluded from the result, and a candidate whose hardcoded layer collides with this
/// instance's is dropped rather than ranked low — a suggestion list is a list of peers an operator may pair with, and a
/// vetoed instance in it would be an invitation that must be refused.
#[must_use]
pub fn rank(own: &Fingerprint, candidates: &[Fingerprint], limit: usize) -> Vec<Suggestion> {
    let mut ranked: Vec<Suggestion> = candidates
        .iter()
        .filter(|candidate| candidate.instance_id != own.instance_id)
        .filter(|candidate| collisions(own, candidate).is_empty())
        .filter_map(|candidate| {
            similarity(own, candidate).and_then(|score| {
                (score >= MIN_SIMILARITY).then(|| Suggestion {
                    instance_id: candidate.instance_id.clone(),
                    similarity: score,
                })
            })
        })
        .collect();

    ranked.sort_by(|a, b| {
        b.similarity
            .partial_cmp(&a.similarity)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.instance_id.cmp(&b.instance_id))
    });
    ranked.truncate(limit.min(MAX_SUGGESTIONS));
    ranked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taste_fingerprint::build;
    use std::collections::BTreeMap;

    fn vector(pairs: &[(&str, f64)]) -> BTreeMap<String, f64> {
        pairs.iter().map(|(tag, weight)| ((*tag).to_owned(), *weight)).collect()
    }

    fn prints(instance: &str, tags: &[(&str, f64)], enforced: &[&str]) -> Fingerprint {
        let mut fingerprint = build(instance, "2026-11-07T00:00:00Z", &vector(tags));
        fingerprint.enforced = enforced.iter().map(|rule| (*rule).to_owned()).collect();
        fingerprint
    }

    fn own() -> Fingerprint {
        prints(
            "https://this.example",
            &[("angst", 10.0), ("fluff", 4.0), ("au", 6.0), ("hurt", 3.0)],
            &["csam"],
        )
    }

    #[test]
    fn a_collision_vetoes_a_perfect_score() {
        let twin = prints(
            "https://lax.example",
            &[("angst", 10.0), ("fluff", 4.0), ("au", 6.0), ("hurt", 3.0)],
            &[],
        );
        // Without the veto this pair is the single best candidate in any list.
        let verdict = assess(&own(), &twin);
        match &verdict {
            Verdict::Collision {
                rule,
                rules,
                enforced_by,
                ..
            } => {
                assert_eq!(rule, "csam");
                assert_eq!(*rules, 1);
                assert_eq!(*enforced_by, "this instance", "the rule is ours and not theirs");
            }
            other => panic!("expected a collision, got {other:?}"),
        }
        assert!(verdict.similarity().is_none(), "a veto has no score to argue with");
        assert!(verdict.reason().contains("different hardcoded layer"), "{}", verdict.reason());

        // And the ranking drops it rather than placing it second.
        let ranked = rank(&own(), std::slice::from_ref(&twin), MAX_SUGGESTIONS);
        assert!(ranked.is_empty(), "{ranked:?}");
        let listed = suggest(&own(), &[twin], MAX_SUGGESTIONS);
        assert_eq!(listed.len(), 1);
        assert!(listed[0].reason().contains("no similarity can make it a peer"));
    }

    #[test]
    fn a_peer_that_enforces_more_than_this_one_collides_too() {
        let strict = prints(
            "https://strict.example",
            &[("angst", 10.0), ("fluff", 4.0), ("au", 6.0), ("hurt", 3.0)],
            &["csam", "gore"],
        );
        match assess(&own(), &strict) {
            Verdict::Collision {
                rule,
                enforced_by,
                rules,
                ..
            } => {
                assert_eq!(rule, "gore");
                assert_eq!(rules, 1);
                assert_eq!(enforced_by, "peer");
            }
            other => panic!("expected a collision, got {other:?}"),
        }
    }

    #[test]
    fn a_shared_rule_is_agreement_even_when_the_lists_differ_around_it() {
        let peer = prints(
            "https://agree.example",
            &[("angst", 10.0), ("fluff", 4.0), ("au", 6.0), ("hurt", 3.0)],
            &["csam"],
        );
        assert!(collisions(&own(), &peer).is_empty());
        assert!(assess(&own(), &peer).is_suggestion());
    }

    #[test]
    fn two_instances_that_enforce_nothing_do_not_collide() {
        let bare = prints("https://bare.example", &[("angst", 10.0)], &[]);
        let other = prints("https://other.example", &[("angst", 9.0)], &[]);
        assert!(collisions(&bare, &other).is_empty(), "absence on both sides is agreement");
    }

    #[test]
    fn an_unmeasured_peer_is_not_maximally_different() {
        let empty = prints("https://empty.example", &[], &["csam"]);
        assert_eq!(similarity(&own(), &empty), None);
        assert_eq!(assess(&own(), &empty).instance_id(), "https://empty.example");
        assert!(rank(&own(), &[empty], MAX_SUGGESTIONS).is_empty());
    }

    #[test]
    fn the_list_puts_what_can_be_acted_on_first() {
        let near = prints(
            "https://near.example",
            &[("angst", 10.0), ("fluff", 4.0), ("au", 6.0), ("hurt", 3.0)],
            &["csam"],
        );
        let blocked = prints(
            "https://blocked.example",
            &[("angst", 10.0), ("fluff", 4.0), ("au", 6.0), ("hurt", 3.0)],
            &[],
        );
        // Shares no tag with this instance at all, so it scores zero rather than merely lower.
        let far = prints("https://far.example", &[("omegaverse", 20.0), ("mpreg", 9.0)], &["csam"]);
        let listed = suggest(&own(), &[far, blocked, near], MAX_SUGGESTIONS);
        assert!(listed[0].is_suggestion());
        assert_eq!(listed[0].instance_id(), "https://near.example");
        assert!(matches!(listed[1], Verdict::Collision { .. }), "{listed:?}");
        assert!(matches!(listed[2], Verdict::NotEnough { .. }), "{listed:?}");
    }

    #[test]
    fn the_instance_is_never_suggested_itself() {
        let listed = suggest(&own(), &[own()], MAX_SUGGESTIONS);
        assert!(listed.is_empty());
    }
}
