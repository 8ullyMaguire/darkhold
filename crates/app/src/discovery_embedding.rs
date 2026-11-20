//! M64-14 — semantic rerank of the served discovery feed.
//!
//! The policy lives in `darkhold_domain::embeddings`; this module only supplies
//! the reader text, the candidate texts and the configured provider. Every
//! failure returns the caller's order, because a reranker that can empty or
//! error a feed is worse than no reranker.

use std::collections::HashMap;

use darkhold_db::Database;
use darkhold_domain::discovery::Candidate;
use darkhold_domain::embeddings::{
    rank_with_summary_similarity, EmbeddingInput, EmbeddingProvider, EmbeddingSource,
};
use serde_json::Value;

/// Rerank `ranked` by similarity to the signed-in reader's fandom signals.
///
/// No-op when the provider is unavailable, the reader has no taste profile, or
/// any database read fails. The provider itself turns request failures back
/// into the caller's order inside `rank_with_summary_similarity`.
pub async fn rerank_with_provider<P: EmbeddingProvider>(
    db: &Database,
    account: &str,
    ranked: Vec<Candidate>,
    provider: &P,
) -> Vec<Candidate> {
    if ranked.len() < 2 || !provider.available() {
        return ranked;
    }
    let profile = match darkhold_db::discovery::taste_profile_for(db, account).await {
        Ok(Some(profile)) => profile,
        Ok(None) => return ranked,
        Err(error) => {
            tracing::warn!(%error, "taste profile unavailable; serving the ranked feed");
            return ranked;
        }
    };
    let Some(reader_text) = reader_text(&profile.signals) else {
        return ranked;
    };

    let ids: Vec<String> = ranked
        .iter()
        .map(|candidate| candidate.work_id.to_string())
        .collect();
    let details = match darkhold_db::discovery::work_details_for(db, &ids).await {
        Ok(details) => details,
        Err(error) => {
            tracing::warn!(%error, "work titles unavailable; serving the ranked feed");
            return ranked;
        }
    };
    let summaries = match darkhold_db::discovery::work_summaries_for(db, &ids).await {
        Ok(summaries) => summaries,
        Err(error) => {
            tracing::warn!(%error, "work summaries unavailable; serving the ranked feed");
            return ranked;
        }
    };

    let texts: Vec<EmbeddingInput> = ids
        .iter()
        .map(|id| {
            let title = details.get(id).map(|(title, _)| title.as_str()).unwrap_or("");
            let summary = summaries.get(id).map(String::as_str).unwrap_or("");
            EmbeddingInput {
                text: format!("{title}\n{summary}"),
                source: EmbeddingSource::Summary,
            }
        })
        .collect();
    let id_refs: Vec<&str> = ids.iter().map(String::as_str).collect();
    let order = rank_with_summary_similarity(&reader_text, &id_refs, &texts, provider, true).await;

    let mut by_id: HashMap<String, Candidate> = ranked
        .into_iter()
        .map(|candidate| (candidate.work_id.to_string(), candidate))
        .collect();
    let mut reranked = Vec::with_capacity(by_id.len());
    for id in order {
        if let Some(candidate) = by_id.remove(id) {
            reranked.push(candidate);
        }
    }
    reranked.extend(by_id.into_values());
    reranked
}

/// The fandom names a taste profile carries, in its stored order. Both shapes
/// are accepted because the fold has emitted a bare array and an object keyed
/// `fandom` over its life, and a reranker should not depend on which one a
/// reader's profile happens to predate.
fn reader_text(signals: &Value) -> Option<String> {
    let fandoms: Vec<String> = match signals {
        Value::Array(values) => values
            .iter()
            .filter_map(|value| value.as_str().map(str::to_owned))
            .collect(),
        Value::Object(map) => map
            .get("fandom")
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(|value| value.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    if fandoms.is_empty() {
        None
    } else {
        Some(fandoms.join(", "))
    }
}
