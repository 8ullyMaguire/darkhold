//! M69-01 — the instance taste fingerprint, in one shape every sibling can read.
//!
//! Peering starts with a question: *is this instance interested in the same things as mine?* Answering it needs a
//! small, stable, comparable document — not a database dump and not an opaque model. This module builds that
//! document and nothing else, so the same code can serve it from `/.well-known/` here and be compared against
//! another instance's copy without either side learning the other's schema.
//!
//! # The shape is the contract
//!
//! ```json
//! {
//!   "version": 1,
//!   "instance_id": "https://darkhold.example",
//!   "generated_at": "2026-11-05T10:00:00Z",
//!   "tags": [{"tag": "hurt/comfort", "weight": 0.19}, ...]
//! }
//! ```
//!
//! Four rules make two fingerprints comparable:
//!
//! * **Top-N, weight-ordered, then alphabetical.** A fingerprint whose order depends on hash iteration cannot be
//!   diffed, and cosine similarity over two differently ordered lists is the same number computed twice as slowly.
//! * **Normalised to sum 1.** An instance with ten readers and one with ten thousand then differ in *what* they
//!   like, not in *how much data* they have.
//! * **Rounded to six decimals.** Floating-point noise in the last bits is not information, and it makes two
//!   instances that agree exactly compare unequal.
//! * **No reader-level data.** A tag weight is an aggregate over the whole instance; the fingerprint cannot be
//!   used to look anybody up, which is what makes publishing it safe.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// The shape version. A sibling that does not know this number ignores the document rather than guessing.
pub const VERSION: u32 = 1;

/// The most tags a fingerprint carries. Enough to characterise a taste profile, few enough that two instances
/// are comparing summaries rather than corpora.
pub const MAX_TAGS: usize = 64;

/// One tag and its share of the instance's taste.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TagWeight {
    pub tag: String,
    pub weight: f64,
}

/// The published document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fingerprint {
    pub version: u32,
    /// The instance's canonical base URL — the same string its ActivityPub actor uses.
    pub instance_id: String,
    /// When the numbers behind this were computed, not when they were served.
    pub generated_at: String,
    pub tags: Vec<TagWeight>,
    /// The rules this instance enforces unconditionally — its hardcoded layer, **as declared** (M69-02).
    ///
    /// Absent means "this instance publishes nothing about it", which is not the same as "enforces nothing": the first is
    /// a gap in the document and the second is a claim. M69-02's veto only fires on a rule one side declares and the other
    /// does not, so an old peer whose document predates this field collides with nothing — the honest reading of a
    /// document that does not answer the question is that it does not answer it.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub enforced: BTreeSet<String>,
}

impl Fingerprint {
    /// The `content-type` a sibling should expect.
    #[must_use]
    pub fn content_type() -> &'static str {
        "application/json"
    }
}

/// Build a fingerprint from a raw tag vector.
///
/// `vector` is whatever the instance's taste computation produced — tag to score. Scores may be negative (a tag
/// the instance reliably dislikes); they are clamped at zero, because a fingerprint is a statement about interest
/// and a negative weight would make two instances that both dislike a tag look *similar* on it.
#[must_use]
pub fn build(instance_id: &str, generated_at: &str, vector: &BTreeMap<String, f64>) -> Fingerprint {
    let mut positive: Vec<(String, f64)> = vector
        .iter()
        .filter(|(_, weight)| weight.is_finite())
        .map(|(tag, weight)| (tag.trim().to_lowercase(), weight.max(0.0)))
        .filter(|(tag, weight)| !tag.is_empty() && *weight > 0.0)
        .collect();

    // One tag spelled twice after lowercasing is one tag, and its weights add.
    let mut merged: BTreeMap<String, f64> = BTreeMap::new();
    for (tag, weight) in positive.drain(..) {
        *merged.entry(tag).or_insert(0.0) += weight;
    }

    let total: f64 = merged.values().sum();
    let mut tags: Vec<TagWeight> = if total > 0.0 {
        merged
            .into_iter()
            .map(|(tag, weight)| TagWeight {
                tag,
                weight: round6(weight / total),
            })
            .collect()
    } else {
        Vec::new()
    };

    // Heaviest first, then alphabetical: a stable order is what makes two documents diffable, and the tie-break
    // has to be by name rather than by insertion because a hash map has no insertion order.
    tags.sort_by(|a, b| {
        b.weight
            .partial_cmp(&a.weight)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.tag.cmp(&b.tag))
    });
    tags.truncate(MAX_TAGS);

    Fingerprint {
        version: VERSION,
        instance_id: instance_id.trim_end_matches('/').to_owned(),
        generated_at: generated_at.to_owned(),
        tags,
        // Declared by the caller, never inferred here: an instance's hardcoded layer is a policy statement, and inferring it
        // from what the instance happens to have served would make the veto depend on a sample.
        enforced: BTreeSet::new(),
    }
}

/// The same document, with the rules this instance enforces declared alongside the taste (M69-02).
#[must_use]
pub fn build_with_rules(
    instance_id: &str,
    generated_at: &str,
    vector: &BTreeMap<String, f64>,
    enforced: impl IntoIterator<Item = String>,
) -> Fingerprint {
    let mut fingerprint = build(instance_id, generated_at, vector);
    fingerprint.enforced = enforced
        .into_iter()
        .map(|rule| rule.trim().to_lowercase())
        .filter(|rule| !rule.is_empty())
        .collect();
    fingerprint
}

/// Cosine similarity over two fingerprints, in `0..=1`.
///
/// This is the question peering asks (M69-02). It is computed here rather than by a reader of the JSON because
/// two instances computing it differently is how "we are similar" stops meaning anything.
///
/// Returns `None` when either side carries no tags: similarity to an empty profile is not zero, it is undefined,
/// and a caller that treats it as zero will suggest peering between an instance and a blank document.
#[must_use]
pub fn similarity(left: &Fingerprint, right: &Fingerprint) -> Option<f64> {
    if left.tags.is_empty() || right.tags.is_empty() {
        return None;
    }
    let left_map: BTreeMap<&str, f64> = left
        .tags
        .iter()
        .map(|row| (row.tag.as_str(), row.weight))
        .collect();
    let right_map: BTreeMap<&str, f64> = right
        .tags
        .iter()
        .map(|row| (row.tag.as_str(), row.weight))
        .collect();

    let dot: f64 = left_map
        .iter()
        .filter_map(|(tag, weight)| right_map.get(tag).map(|other| weight * other))
        .sum();
    let left_norm = left_map.values().map(|w| w * w).sum::<f64>().sqrt();
    let right_norm = right_map.values().map(|w| w * w).sum::<f64>().sqrt();
    if left_norm <= 0.0 || right_norm <= 0.0 {
        return None;
    }
    Some(round6((dot / (left_norm * right_norm)).clamp(0.0, 1.0)))
}

fn round6(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}
