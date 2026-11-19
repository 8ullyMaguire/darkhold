//! M64-13's `engagement_overlap` door: a public, floor-suppressed count of readers shared by two works.
//!
//! Reader clusters are derived from signed-in reading sessions with a real duration. Only the size of the intersection is read
//! from the database, and it passes through the instance's k-anonymity floor before it becomes a response. No reader id, session
//! id or below-floor number is published.

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::http::ApiResult;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/works/{work_id}/engagement-overlap", get(overlap))
}

#[derive(Debug, Deserialize)]
struct Params {
    with: String,
    #[serde(default = "default_days")]
    since_days: i64,
}

fn default_days() -> i64 {
    30
}

#[derive(Debug, Serialize)]
struct Response {
    work_id: String,
    with: String,
    overlap: darkhold_domain::analytics::Reported,
}

async fn overlap(
    State(state): State<AppState>,
    Path(work_id): Path<String>,
    Query(params): Query<Params>,
) -> ApiResult<Json<Response>> {
    let not_found =
        || crate::http::ApiError(darkhold_domain::AppError::NotFound { resource: "work" });
    let _left: darkhold_domain::WorkId = work_id.parse().map_err(|_| not_found())?;
    let _right: darkhold_domain::WorkId = params.with.parse().map_err(|_| not_found())?;
    let now = time::OffsetDateTime::now_utc();
    let since = now - time::Duration::days(params.since_days.clamp(1, 365));
    let format = time::format_description::well_known::Rfc3339;
    let options = darkhold_db::engagement_overlap::Options {
        since: since.format(&format).unwrap_or_default(),
        until: now.format(&format).unwrap_or_default(),
    };
    let count = darkhold_db::engagement_overlap::reader_overlap(
        state.db(),
        &work_id,
        &params.with,
        &options,
    )
    .await?;
    Ok(Json(Response {
        work_id,
        with: params.with,
        overlap: darkhold_domain::stats::report(count, darkhold_domain::analytics::Subject::Other),
    }))
}
