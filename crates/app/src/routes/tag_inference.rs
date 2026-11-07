//! Operator doors for the tag-inference pass and its coverage report (M63-32).
//!
//! ```text
//! GET  /operator/tag-inference/coverage   per source: items, tagged, with likes, both
//! POST /operator/tag-inference/run        one pass over up to `limit` untagged imported works
//! ```
//!
//! Both are operator-only: the pass writes rows, and the report is a statement about the instance's own data that no reader has a
//! reason to see. The pass writes **proposals** — `inferred + unconfirmed`, invisible to readers until confirmed — so an operator can
//! inspect `assertions_for_work` before anything becomes a fact.

use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use crate::http::ApiResult;
use crate::state::AppState;

/// The tag-inference routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/operator/tag-inference/coverage", get(coverage))
        .route("/operator/tag-inference/run", post(run))
}

/// Per-source coverage.
async fn coverage(
    State(state): State<AppState>,
    crate::auth::RequireSession(user): crate::auth::RequireSession,
) -> ApiResult<Json<serde_json::Value>> {
    crate::routes::experiment_sweep::require_operator(&state, &user)?;
    let rows = darkhold_db::tag_inference::coverage(state.db()).await?;
    Ok(Json(serde_json::json!({ "sources": rows })))
}

#[derive(Deserialize)]
struct RunRequest {
    #[serde(default = "default_limit")]
    limit: i64,
}

fn default_limit() -> i64 {
    500
}

/// One inference pass.
async fn run(
    State(state): State<AppState>,
    crate::auth::RequireSession(user): crate::auth::RequireSession,
    Query(query): Query<RunRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    crate::routes::experiment_sweep::require_operator(&state, &user)?;
    // Bounded: an unbounded pass over a million rows is a table nobody can review, and the point is to run it, look, and run it
    // again. The cap is also what keeps one HTTP request from doing an hour of work.
    let limit = query.limit.clamp(1, 5_000);
    let report = darkhold_db::tag_inference::propose_from_text(state.db(), limit).await?;
    Ok(Json(serde_json::json!({
        "examined": report.examined,
        "proposed": report.proposed,
        "note": "proposals are inferred and unconfirmed; readers do not see them until a confirmation is written",
    })))
}
