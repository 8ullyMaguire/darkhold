//! M64-11's `story.lists_containing` door: the public reading lists holding one work.
//!
//! The answer is the same for every caller, so the route is anonymous. It never names a private list: an adjacency that a reader
//! cannot follow is not a discovery signal, and even a count would disclose existence.

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use darkhold_domain::user_list::UserList;
use serde::{Deserialize, Serialize};

use crate::http::ApiResult;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/works/{work_id}/lists", get(lists_containing))
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
struct ListsResponse {
    work_id: String,
    lists: Vec<UserList>,
}

async fn lists_containing(
    State(state): State<AppState>,
    Path(work_id): Path<String>,
    Query(params): Query<Params>,
) -> ApiResult<Json<ListsResponse>> {
    let lists =
        darkhold_db::user_list::lists_containing(state.db(), &work_id, params.limit).await?;
    Ok(Json(ListsResponse { work_id, lists }))
}
