//! The operator door for per-source engagement (M63-37).
//!
//! ```text
//! GET /operator/library-items/:id/engagement          every metric the archive reported
//! PUT /operator/library-items/:id/engagement/:metric  record one, as the archive reported it
//! ```
//!
//! A route rather than only a db function because the importer that reads a source page **is not in this binary**: the scrapers run
//! elsewhere. Without a door, this table would be a store nothing can write to, and the row would be "built" in the sense that a
//! function exists and nothing calls it. The door is operator-only: an imported item belongs to the account that imported it, and a
//! metric is a claim about somebody's work that no reader writes.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, put};
use axum::{Json, Router};
use serde::Deserialize;

use crate::http::{ApiError, ApiResult};
use crate::state::AppState;
use darkhold_domain::AppError;

/// The engagement routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/operator/library-items/{id}/engagement", get(list))
        .route(
            "/operator/library-items/{id}/engagement/{metric}",
            put(record_one),
        )
}

/// What an archive said about one imported item.
async fn list(
    State(state): State<AppState>,
    crate::auth::RequireSession(user): crate::auth::RequireSession,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    crate::routes::experiment_sweep::require_operator(&state, &user)?;
    let readings = darkhold_db::source_engagement::readings(state.db(), &id).await?;
    // The ratio is computed here rather than by a `ROUND(likes / views)` in a page's SQL, because **the denominator's
    // provenance is the rule**: a source's endorsements over *that same source's* views, `null` when either is missing or
    // views is zero. An inline division returns 0 for "no views" and an error or a zero for "not published", and both are
    // wrong in a way that reads as a real number. (M70-13)
    let likes_per_view = match (
        darkhold_db::source_engagement::likes_per_view(
            state.db(),
            &id,
            darkhold_db::source_engagement::Metric::Likes,
            darkhold_db::source_engagement::Metric::Views,
        )
        .await,
        darkhold_db::source_engagement::likes_per_view(
            state.db(),
            &id,
            darkhold_db::source_engagement::Metric::Kudos,
            darkhold_db::source_engagement::Metric::Hits,
        )
        .await,
    ) {
        (Ok(Some(ratio)), _) => Some(ratio),
        (_, Ok(Some(ratio))) => Some(ratio),
        _ => None,
    };
    Ok(Json(serde_json::json!({
        "metrics": readings,
        // `null` is a real answer: the archive did not publish one half, or published no views. `0.0` would be a claim
        // that nobody liked it.
        "likes_per_view": likes_per_view,
    })))
}

#[derive(Deserialize)]
struct RecordRequest {
    value: i64,
    /// When the source published the number. Required: an engagement count with no date cannot be told apart from a stale one, and
    /// a rate computed from a counter of unknown age is a rate of unknown age.
    reported_at: String,
}

/// Record one metric.
async fn record_one(
    State(state): State<AppState>,
    crate::auth::RequireSession(user): crate::auth::RequireSession,
    Path((id, metric)): Path<(String, String)>,
    Json(body): Json<RecordRequest>,
) -> ApiResult<StatusCode> {
    crate::routes::experiment_sweep::require_operator(&state, &user)?;
    // Refused by name, not ignored: a metric this build does not know would be stored under a key nothing reads, and the caller would
    // believe the number had landed.
    let metric = darkhold_db::source_engagement::Metric::parse(&metric).ok_or_else(|| {
        ApiError(AppError::Validation {
            message: format!(
                "`{metric}` is not an engagement metric this build knows; expected one of {}",
                darkhold_db::source_engagement::Metric::all()
                    .iter()
                    .map(|m| m.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            field_errors: Default::default(),
        })
    })?;
    darkhold_db::source_engagement::record(state.db(), &id, metric, body.value, &body.reported_at)
        .await
        .map_err(|error| {
            ApiError(AppError::Validation {
                message: error.to_string(),
                field_errors: Default::default(),
            })
        })?;
    Ok(StatusCode::NO_CONTENT)
}
