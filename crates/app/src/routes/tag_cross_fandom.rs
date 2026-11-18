//! M64-08's trope surface: the same declared map, reached by "this trope in a neighbour fandom".
//!
//! A trope alone does not name a source fandom, so `from` is explicit. The answer is the operator's map version, its
//! declared neighbours, and published/public works carrying the same trope in those neighbours — identical for every caller.

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::http::ApiResult;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/tags/{tag}/cross-fandom", get(cross_fandom))
}

#[derive(Debug, Deserialize)]
struct Params {
    from: String,
    #[serde(default = "default_limit")]
    limit: i64,
}

fn default_limit() -> i64 {
    20
}

#[derive(Debug, Serialize)]
struct CrossFandomResponse {
    tag: String,
    from: String,
    graph_version: String,
    neighbours: Vec<darkhold_domain::fandom_graph::Neighbour>,
    works: Vec<darkhold_db::fandom_graph::RelatedWork>,
}

async fn cross_fandom(
    State(state): State<AppState>,
    Path(tag): Path<String>,
    Query(params): Query<Params>,
) -> ApiResult<Json<CrossFandomResponse>> {
    let graph = &state.config().fandom_graph;
    let neighbours = graph.neighbours(&params.from);
    let weights: Vec<(String, f64)> = neighbours
        .iter()
        .map(|neighbour| (neighbour.name.clone(), neighbour.weight))
        .collect();
    let limit = params.limit.clamp(0, 100);
    let works =
        darkhold_db::fandom_graph::tag_cross_fandom_works(state.db(), &tag, &weights, &[], limit)
            .await?;
    Ok(Json(CrossFandomResponse {
        tag,
        from: params.from,
        graph_version: graph.version.clone(),
        neighbours,
        works,
    }))
}
