//! M64-08's door — a fandom's declared neighbours, and the works they reach.
//!
//! ```text
//! GET /api/v1/fandoms/:fandom/related?limit=
//! ```
//!
//! The map is the operator's, so the answer is the same for every caller: no session, no personalisation, `Audience::Anonymous`. That is
//! also why the response names the **map version** — a reader looking at "Ward because of Worm" is being told something an operator
//! decided on a date, and a page that cannot say which map it used invites the reader to think the site inferred it.
//!
//! An unmapped fandom is answered, not refused: `neighbours: []`, `works: []`, and the version. "This instance has not declared anything
//! next to that fandom" is a true answer, and a 404 would make it look like the fandom does not exist — which is a different fact the
//! catalogue route owns.

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::http::ApiResult;
use crate::state::AppState;

/// The route.
pub fn router() -> Router<AppState> {
    Router::new().route("/api/v1/fandoms/{fandom}/related", get(related))
}

#[derive(Debug, Deserialize)]
struct RelatedParams {
    /// How many works to return. The neighbours are always all of them: they are a claim about the map, not a page of it.
    limit: Option<i64>,
}

#[derive(Debug, Serialize)]
struct RelatedResponse {
    /// The fandom asked about, as the caller spelled it.
    fandom: String,
    /// The map's identity, so a page can say which one it used.
    graph_version: String,
    /// Declared neighbours, strongest first, each with the weight the operator gave it.
    neighbours: Vec<darkhold_domain::fandom_graph::Neighbour>,
    /// Works in those fandoms, strongest edge first. Empty when nothing is readable there.
    works: Vec<darkhold_db::fandom_graph::RelatedWork>,
}

/// A fandom's neighbours and the works they reach.
async fn related(
    State(state): State<AppState>,
    Path(fandom): Path<String>,
    Query(params): Query<RelatedParams>,
) -> ApiResult<Json<RelatedResponse>> {
    let graph = &state.config().fandom_graph;
    let neighbours = graph.neighbours(&fandom);
    let weights: Vec<(String, f64)> = neighbours
        .iter()
        .map(|neighbour| (neighbour.name.clone(), neighbour.weight))
        .collect();
    // A limit of zero is a caller asking for the map and not the works; refusing it would be pedantry about a readable request.
    let limit = params.limit.unwrap_or(20).clamp(0, 100);
    let works = darkhold_db::fandom_graph::related_works(state.db(), &weights, &[], limit).await?;
    Ok(Json(RelatedResponse {
        fandom,
        graph_version: graph.version.clone(),
        neighbours,
        works,
    }))
}
