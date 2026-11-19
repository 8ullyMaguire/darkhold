//! M64-12's `story.also_enjoyed` door: works read after this one by readers who spent real time on both.
//!
//! The answer is an aggregate with a minimum-reader floor. It is public because it names no reader, but it never reports an edge
//! below the floor: an empty list means unmeasured, not "nobody enjoyed this".

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::http::ApiResult;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/works/{work_id}/also-enjoyed", get(also_enjoyed))
}

#[derive(Debug, Deserialize)]
struct Params {
    #[serde(default = "default_days")]
    since_days: i64,
    #[serde(default = "default_limit")]
    limit: i64,
    #[serde(default = "default_min_readers")]
    min_readers: i64,
}

fn default_days() -> i64 {
    30
}
fn default_limit() -> i64 {
    20
}
fn default_min_readers() -> i64 {
    3
}

#[derive(Debug, Serialize)]
struct CoReadWork {
    work_id: String,
    readers: i64,
}

#[derive(Debug, Serialize)]
struct Response {
    work_id: String,
    works: Vec<CoReadWork>,
}

async fn also_enjoyed(
    State(state): State<AppState>,
    Path(work_id): Path<String>,
    Query(params): Query<Params>,
) -> ApiResult<Json<Response>> {
    let _parsed: darkhold_domain::WorkId = work_id.parse().map_err(|_| {
        crate::http::ApiError(darkhold_domain::AppError::NotFound { resource: "work" })
    })?;
    let now = time::OffsetDateTime::now_utc();
    let since = now - time::Duration::days(params.since_days.clamp(1, 365));
    let format = time::format_description::well_known::Rfc3339;
    let options = darkhold_db::co_read::Options {
        since: since.format(&format).unwrap_or_default(),
        until: now.format(&format).unwrap_or_default(),
        min_readers: params.min_readers.clamp(1, 1_000),
        limit: params.limit,
    };
    let found = darkhold_db::co_read::also_enjoyed(state.db(), &work_id, &options).await?;
    Ok(Json(Response {
        work_id,
        works: found
            .into_iter()
            .map(|(work_id, readers)| CoReadWork { work_id, readers })
            .collect(),
    }))
}
