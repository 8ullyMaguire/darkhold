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
}

pub fn router() -> Router<AppState> {
    Router::new().route("/api/v1/facets", get(facets))
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
        tag: params.tag.map(|tag| tag.trim().to_owned()).filter(|tag| !tag.is_empty()),
        title_contains: params
            .q
            .map(|text| text.trim().to_owned())
            .filter(|text| !text.is_empty()),
    };

    let answers = facets::all_facets(state.db(), &filter)
        .await
        .map_err(|error| ApiError(AppError::internal("reading facets", error)))?;
    let answers: Vec<_> = answers
        .into_iter()
        .filter(|answer| params.facet.as_deref().is_none_or(|name| name == answer.facet))
        .collect();

    Ok(Json(json!({
        "filter": {
            "tag": filter.tag,
            "q": filter.title_contains,
        },
        "facets": answers,
    })))
}
