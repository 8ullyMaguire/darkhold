//! Operator door for the automatic work-lifecycle sweep (M45-76).
//!
//! Default body is a dry run: an operator can always inspect what the sweep
//! would do, even when automatic abandonment is switched off in configuration.

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;

use crate::auth::RequireSession;
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct SweepBody {
    #[serde(default = "default_true")]
    pub dry_run: bool,
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub cursor: Option<String>,
}

fn default_true() -> bool {
    true
}

fn default_limit() -> i64 {
    500
}

pub async fn sweep(
    RequireSession(user): RequireSession,
    State(state): State<AppState>,
    Json(body): Json<SweepBody>,
) -> ApiResult<Json<crate::work_lifecycle::SweepReport>> {
    crate::routes::discovery::require_operator(&state, &user)?;
    if !body.dry_run && !state.config().works.auto_abandon {
        return Err(ApiError(darkhold_domain::AppError::field(
            "dry_run",
            "automatic abandonment is disabled by [works].auto_abandon = false; a write would contradict the configured state",
        )));
    }
    let report =
        crate::work_lifecycle::sweep_page(&state, body.cursor.clone(), body.limit, body.dry_run)
            .await?;
    Ok(Json(report))
}

pub fn router() -> Router<AppState> {
    Router::new().route("/lifecycle/sweep", post(sweep))
}
