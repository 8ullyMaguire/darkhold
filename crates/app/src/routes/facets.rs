//! `GET /api/v1/facets` — length, status, rating and cadence for one filtered set (M64-10).

use crate::auth::MaybeSession;
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use darkhold_db::facets::{self, FacetFilter};
use darkhold_domain::error::AppError;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
struct FacetsQuery {
    /// Tag name, exactly as `/tags/{tag}` takes it (spaces and all).
    tag: Option<String>,
    /// Substring of the title.
    q: Option<String>,
    /// Narrow the answer to one facet; absent returns all four.
    facet: Option<String>,
    /// The same word window and rating ceiling `/api/v1/search` takes, so a facet can describe a narrowed catalogue.
    min_words: Option<i64>,
    max_words: Option<i64>,
    max_rating: Option<String>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/facets", get(facets))
        // M71-12: the word-count distribution under the same filters. A separate door because it answers a different question with a
        // different shape — ten population-equal buckets with their edges, not a list of named values — and because a chart is fetched by
        // pages that never render the other four facets.
        .route("/api/v1/facets/words", get(word_histogram))
}

#[derive(Deserialize)]
struct HistogramQuery {
    /// Tag name, exactly as `/tags/{tag}` takes it.
    tag: Option<String>,
    /// Substring of the title.
    q: Option<String>,
    /// How many buckets. Ten by default, which is what a chart can draw; clamped rather than refused, because a bar count is a
    /// presentation choice and a 4-bucket request is a legitimate one.
    buckets: Option<usize>,
    min_words: Option<i64>,
    max_words: Option<i64>,
    max_rating: Option<String>,
}

/// The word-count histogram (M71-12).
async fn word_histogram(
    State(state): State<AppState>,
    MaybeSession(_viewer): MaybeSession,
    Query(params): Query<HistogramQuery>,
) -> ApiResult<Json<Value>> {
    let filter = FacetFilter {
        tag: params
            .tag
            .map(|tag| tag.trim().to_owned())
            .filter(|tag| !tag.is_empty()),
        title_contains: params
            .q
            .map(|text| text.trim().to_owned())
            .filter(|text| !text.is_empty()),
        tag_kind: None,
        min_words: params.min_words,
        max_words: params.max_words,
        max_rating: params.max_rating,
    };
    let histogram = facets::word_histogram(state.db(), &filter, params.buckets.unwrap_or(10))
        .await
        .map_err(|error| ApiError(AppError::internal("reading the word histogram", error)))?;
    Ok(Json(json!({
        // Named so a page can print what the numbers describe, and so two charts on one screen cannot be confused.
        "unit": "words",
        "buckets": histogram.buckets,
        "measured": histogram.measured,
        "unknown": histogram.unknown,
        "total": histogram.total,
        "when_unknown": histogram.when_unknown,
    })))
}

async fn facets(
    State(state): State<AppState>,
    MaybeSession(_viewer): MaybeSession,
    Query(params): Query<FacetsQuery>,
) -> ApiResult<Json<Value>> {
    if let Some(name) = params.facet.as_deref() {
        if facets::Facet::parse(name).is_none() {
            return Err(ApiError(AppError::field(
                "facet",
                "is one of length, status, rating, cadence",
            )));
        }
    }
    let filter = FacetFilter {
        tag: params
            .tag
            .map(|tag| tag.trim().to_owned())
            .filter(|tag| !tag.is_empty()),
        title_contains: params
            .q
            .map(|text| text.trim().to_owned())
            .filter(|text| !text.is_empty()),
        tag_kind: None,
        min_words: params.min_words,
        max_words: params.max_words,
        max_rating: params.max_rating,
    };

    let (answers, computed_at) = crate::facets_cache::cached(state.db(), &filter)
        .await
        .map_err(|error| ApiError(AppError::internal("reading facets", error)))?;
    let answers: Vec<_> = answers
        .into_iter()
        .filter(|answer| {
            params
                .facet
                .as_deref()
                .is_none_or(|name| name == answer.facet)
        })
        .collect();

    Ok(Json(json!({
        "filter": {
            "tag": filter.tag,
            "q": filter.title_contains,
            "min_words": filter.min_words,
            "max_words": filter.max_words,
            "max_rating": filter.max_rating,
        },
        "facets": answers,
        // M71-11: the numbers are memoised for a minute, so the response says when they were computed instead of implying they are live.
        "computed_at_ms_ago": computed_at.elapsed().as_millis() as u64,
        "cache_ttl_seconds": crate::facets_cache::TTL.as_secs(),
    })))
}
