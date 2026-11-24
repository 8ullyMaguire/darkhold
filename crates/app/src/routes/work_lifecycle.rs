//! Operator door for the automatic work-lifecycle sweep (M45-76).
//!
//! Default body is a dry run: an operator can always inspect what the sweep
//! would do, even when automatic abandonment is switched off in configuration.

use axum::extract::{Path, State};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::RequireSession;
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

use darkhold_domain::error::AppError;

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

#[derive(Debug, Deserialize)]
pub struct CompletionBody {
    pub completion: String,
    #[serde(default)]
    pub note: Option<String>,
}

pub async fn set_completion(
    RequireSession(user): RequireSession,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<CompletionBody>,
) -> ApiResult<Json<Value>> {
    let work_id = crate::routes::works::parse_work_id(&id)?;
    let Some(work) = darkhold_db::content::find_work(state.db(), work_id).await? else {
        return Err(ApiError(AppError::NotFound { resource: "work" }));
    };
    if work.lifecycle != "published" {
        return Err(ApiError(AppError::field(
            "completion",
            "only a published work carries a completion state",
        )));
    }
    if !matches!(body.completion.as_str(), "complete" | "in_progress") {
        return Err(ApiError(AppError::field(
            "completion",
            "a reviewer sets complete or in_progress; abandoned and hiatus are the author's states",
        )));
    }
    let account = user.account_id.to_string();
    let may = darkhold_db::work_lifecycle::may_review_completion(
        state.db(),
        &account,
        state.config().works.completion_review_min_trust,
    )
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    if !may {
        // Name the bar and the level actually held. A bare 403 leaves a TL2
        // reviewer unable to tell a configuration change from a missing grant.
        let required = state.config().works.completion_review_min_trust;
        let level = darkhold_db::governance::trust_for(state.db(), &account)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        return Err(ApiError(AppError::TrustLevelInsufficient {
            level,
            required,
        }));
    }
    if body.completion == work.completion {
        return Ok(Json(json!({
            "work_id": work.id.to_string(),
            "completion": work.completion,
            "changed": false,
        })));
    }
    let now = darkhold_db::identity::now_rfc3339();
    let changed = darkhold_db::work_lifecycle::apply_transition(
        state.db(),
        &work.id.to_string(),
        &work.completion,
        &body.completion,
        darkhold_domain::work_lifecycle::CompletionSource::Review,
        Some(&account),
        body.note.as_deref(),
        None,
        &now,
    )
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    if !changed {
        let current = darkhold_db::content::find_work(state.db(), work_id)
            .await?
            .ok_or_else(|| ApiError(AppError::NotFound { resource: "work" }))?;
        return Ok(Json(json!({
            "work_id": current.id.to_string(),
            "completion": current.completion,
            "changed": false,
        })));
    }
    Ok(Json(json!({
        "work_id": work.id.to_string(),
        "completion": body.completion,
        "changed": true,
    })))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/lifecycle/sweep", post(sweep))
        .route("/works/{id}/completion", post(set_completion))
}
