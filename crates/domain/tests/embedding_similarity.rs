//! M64-14: embeddings are optional and never turn a provider failure into a reader-facing error.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;

use darkhold_domain::embeddings::{
    rank_with_summary_similarity, EmbeddingError, EmbeddingInput, EmbeddingProvider,
    EmbeddingSource,
};

struct Fake {
    available: bool,
    fail: bool,
    vectors: BTreeMap<String, Vec<f32>>,
}

impl EmbeddingProvider for Fake {
    fn available(&self) -> bool {
        self.available
    }
    fn embed<'a>(
        &'a self,
        input: &'a EmbeddingInput,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<f32>, EmbeddingError>> + Send + 'a>> {
        let fail = self.fail;
        let vector = self.vectors.get(&input.text).cloned();
        Box::pin(async move {
            if fail {
                return Err(EmbeddingError::Unavailable);
            }
            vector.ok_or(EmbeddingError::Unavailable)
        })
    }
}

fn provider(available: bool, fail: bool) -> Fake {
    let mut vectors = BTreeMap::new();
    vectors.insert("reader summary".to_string(), vec![1.0, 0.0]);
    vectors.insert("summary".to_string(), vec![1.0, 0.0]);
    vectors.insert("full".to_string(), vec![0.0, 1.0]);
    Fake {
        available,
        fail,
        vectors,
    }
}

#[test]
fn summary_is_the_first_source() {
    let input = EmbeddingInput::select("summary", "full", EmbeddingSource::Summary);
    assert_eq!(input.text, "summary");
    let input = EmbeddingInput::select("", "full", EmbeddingSource::Summary);
    assert_eq!(input.text, "full");
    let input = EmbeddingInput::select("summary", "full", EmbeddingSource::FullText);
    assert_eq!(input.text, "full");
}

#[tokio::test]
async fn similar_summaries_reorder_the_candidates() {
    let ids = ["near", "far"];
    let near = EmbeddingInput::select("summary", "full", EmbeddingSource::Summary);
    let far = EmbeddingInput::select("full", "full", EmbeddingSource::Summary);
    let texts = [near, far];
    let p = provider(true, false);
    let ranked = rank_with_summary_similarity("reader summary", &ids, &texts, &p, true).await;
    assert_eq!(ranked, vec!["near", "far"]);
}

#[tokio::test]
async fn disabled_or_broken_provider_keeps_the_original_order() {
    let ids = ["far", "near"];
    let texts = [
        EmbeddingInput::select("full", "full", EmbeddingSource::Summary),
        EmbeddingInput::select("summary", "full", EmbeddingSource::Summary),
    ];
    for (enabled, p) in [
        (false, provider(true, false)),
        (true, provider(false, false)),
        (true, provider(true, true)),
    ] {
        assert_eq!(
            rank_with_summary_similarity("reader summary", &ids, &texts, &p, enabled).await,
            vec!["far", "near"]
        );
    }
}
