//! M64-14 — provider-gated embedding similarity.
//!
//! Summary is the first text source because it is the cheap, already-authored description of a work; full text is
//! used only when a caller asks for it or when the summary is empty. The arithmetic lives in `polaris-affinity`: a
//! dense vector is indexed by dimension and handed to the same kernel that scores tag counts, so darkhold has no
//! second dot product and no second clamp.
//!
//! Nothing in this module is fatal. A disabled gate, unavailable provider, failed request, malformed vector or
//! zero-norm embedding all answer `None` or leave the caller's order untouched. Ranking is an enhancement, not a
//! dependency: a reader whose provider is down gets the same feed they got before the feature existed.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;

/// Which text a caller wants embedded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingSource {
    Summary,
    FullText,
}

/// One text to embed, after the source rule has been applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingInput {
    pub text: String,
    pub source: EmbeddingSource,
}

impl EmbeddingInput {
    /// Summary first; full text only for an explicit full-text request or an empty summary.
    #[must_use]
    pub fn select(summary: &str, full_text: &str, source: EmbeddingSource) -> Self {
        let (text, source) = match source {
            EmbeddingSource::Summary if !summary.trim().is_empty() => {
                (summary.to_string(), EmbeddingSource::Summary)
            }
            EmbeddingSource::Summary => (full_text.to_string(), EmbeddingSource::FullText),
            EmbeddingSource::FullText => (full_text.to_string(), EmbeddingSource::FullText),
        };
        Self { text, source }
    }
}

/// Why a provider could not answer. Callers treat every case as "no embedding", never as a reader-facing error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbeddingError {
    Unavailable,
    InvalidVector,
}

/// A provider that can embed text for this task. `available` is separate from `embed` so a configured-but-unreachable
/// provider degrades without a request and a request failure degrades without a panic.
pub trait EmbeddingProvider: Send + Sync {
    fn available(&self) -> bool;
    fn embed<'a>(
        &'a self,
        input: &'a EmbeddingInput,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<f32>, EmbeddingError>> + Send + 'a>>;
}

/// Cosine similarity of two embeddings, `None` when either vector is empty, malformed or zero-norm.
#[must_use]
pub fn similarity(left: &[f32], right: &[f32]) -> Option<f64> {
    if left.len() != right.len() || left.is_empty() {
        return None;
    }
    let left: BTreeMap<usize, f64> = left
        .iter()
        .enumerate()
        .filter_map(|(index, value)| {
            let value = f64::from(*value);
            value.is_finite().then_some((index, value))
        })
        .collect();
    let right: BTreeMap<usize, f64> = right
        .iter()
        .enumerate()
        .filter_map(|(index, value)| {
            let value = f64::from(*value);
            value.is_finite().then_some((index, value))
        })
        .collect();
    polaris_affinity::similarity::cosine_counts(&left, &right)
}

/// Re-rank `ids` by similarity of their chosen text to the reader's text.
///
/// The gate is explicit: `enabled == false` returns the caller's order without asking the provider anything. Every
/// provider failure leaves the order untouched. Ties keep original order, so embedding never reshuffles a row for no
/// measured reason.
pub async fn rank_with_summary_similarity<'a, P>(
    reader_text: &'a str,
    ids: &'a [&'a str],
    texts: &'a [EmbeddingInput],
    provider: &'a P,
    enabled: bool,
) -> Vec<&'a str>
where
    P: EmbeddingProvider,
{
    if !enabled || !provider.available() || ids.len() != texts.len() {
        return ids.to_vec();
    }
    let reader = EmbeddingInput::select(reader_text, reader_text, EmbeddingSource::Summary);
    let Ok(reader_vector) = provider.embed(&reader).await else {
        return ids.to_vec();
    };
    let mut scored: Vec<(usize, f64)> = Vec::with_capacity(ids.len());
    for (index, text) in texts.iter().enumerate() {
        let Ok(vector) = provider.embed(text).await else {
            return ids.to_vec();
        };
        let Some(score) = similarity(&reader_vector, &vector) else {
            return ids.to_vec();
        };
        scored.push((index, score));
    }
    scored.sort_by(|(left_index, left), (right_index, right)| {
        right
            .total_cmp(left)
            .then_with(|| left_index.cmp(right_index))
    });
    scored.into_iter().map(|(index, _)| ids[index]).collect()
}
