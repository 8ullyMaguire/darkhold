//! M69-02 (first slice) — which peers are worth suggesting, decided once.
//!
//! Peering is a suggestion, so the ranking has to be a pure function of what each side published: no session, no
//! hidden state, and no random tie-break that makes the same list come back in a different order twice.
//!
//! Four rules, each of which exists because the alternative is a bad suggestion:
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

use crate::taste_fingerprint::{similarity, Fingerprint};

/// The floor a candidate must clear to be worth showing. Chosen to be visibly above "shares one popular tag":
/// two profiles that agree on one tag out of twenty land far below this.
pub const MIN_SIMILARITY: f64 = 0.25;

/// The most suggestions one call returns. A suggestion surface is an invitation, not a directory.
pub const MAX_SUGGESTIONS: usize = 10;

/// One suggested peer.
#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub instance_id: String,
    pub similarity: f64,
}

/// Rank candidate fingerprints against this instance's own.
///
/// `candidates` are fingerprints fetched from other instances; `own` is this instance's published one. The own
/// fingerprint's `instance_id` is excluded from the result.
#[must_use]
pub fn rank(own: &Fingerprint, candidates: &[Fingerprint], limit: usize) -> Vec<Suggestion> {
    let mut ranked: Vec<Suggestion> = candidates
        .iter()
        .filter(|candidate| candidate.instance_id != own.instance_id)
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
