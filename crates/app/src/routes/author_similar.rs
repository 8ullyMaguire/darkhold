//! M64-08's author surface: authors reached through the fandoms the named author writes in.
//!
//! The answer is the operator's declared map, never an inference from a reader's history. An author with no published/public
//! works has no declared source fandom and therefore an empty neighbour list, answered with the map version rather than a 404.

use std::collections::BTreeMap;

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::http::ApiResult;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/authors/{author}/similar", get(similar))
}

#[derive(Debug, Deserialize)]
struct Params {
    #[serde(default = "default_limit")]
    limit: i64,
}

fn default_limit() -> i64 {
    20
}

#[derive(Debug, Serialize)]
struct SimilarResponse {
    author: String,
    graph_version: String,
    neighbours: Vec<darkhold_domain::fandom_graph::Neighbour>,
    authors: Vec<darkhold_db::fandom_graph::SimilarAuthor>,
}

async fn similar(
    State(state): State<AppState>,
    Path(author): Path<String>,
    Query(params): Query<Params>,
) -> ApiResult<Json<SimilarResponse>> {
    let graph = &state.config().fandom_graph;
    let source_fandoms = darkhold_db::fandom_graph::author_fandoms(state.db(), &author).await?;
    let mut neighbours: BTreeMap<String, darkhold_domain::fandom_graph::Neighbour> =
        BTreeMap::new();
    for fandom in source_fandoms {
        for neighbour in graph.neighbours(&fandom) {
            let key = neighbour.name.to_lowercase();
            let strongest = neighbours
                .get(&key)
                .is_none_or(|current| neighbour.weight > current.weight);
            if strongest {
                neighbours.insert(key, neighbour);
            }
        }
    }
    let declared: Vec<darkhold_domain::fandom_graph::Neighbour> =
        neighbours.into_values().collect();
    let weights: Vec<(String, f64)> = declared
        .iter()
        .map(|neighbour| (neighbour.name.clone(), neighbour.weight))
        .collect();
    let limit = params.limit.clamp(0, 100);
    let authors =
        darkhold_db::fandom_graph::similar_authors(state.db(), &author, &weights, limit).await?;
    Ok(Json(SimilarResponse {
        author,
        graph_version: graph.version.clone(),
        neighbours: declared,
        authors,
    }))
}
