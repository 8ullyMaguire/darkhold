//! Trope coverage: which families a work names and which it never names (M69-18).
//!
//! ```text
//! GET  /works/:id/trope-coverage        the checklist (404 unless the operator switched it on)
//! GET  /operator/trope-clusters         every cluster, priority-ordered
//! POST /operator/trope-clusters         make one
//! POST /operator/trope-clusters/:slug/nodes/:nodeId
//! ```
//!
//! The public route is **advisory and silent by default**: with `taxonomy.trope_coverage` off it answers `404`, the same shape as a
//! module that was never installed. "The operator has not published this checklist" is not an error a reader can act on, and a
//! `403` would tell them a door exists that they are not allowed through.
//!
//! The operator routes stay available with the switch off — building the clusters is how an operator decides whether to turn it on.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use crate::http::{ApiError, ApiResult};
use crate::state::AppState;
use darkhold_domain::AppError;

/// The trope-coverage routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/works/{id}/trope-coverage", get(for_work))
        .route(
            "/operator/trope-clusters",
            get(list_clusters).post(create_cluster),
        )
        .route(
            "/operator/trope-clusters/{slug}/nodes/{node_id}",
            post(add_node),
        )
}

/// One work's checklist.
async fn for_work(
    State(state): State<AppState>,
    // The same checklist answers with or without a session; `MaybeSession` rather than `Anonymous` because a future operator-only
    // detail (a curator note on a gap) would hang off the viewer, and a public door must say it may see one.
    _viewer: crate::auth::MaybeSession,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    if !state.config().taxonomy.trope_coverage {
        return Err(ApiError(AppError::NotFound {
            resource: "trope coverage",
        }));
    }
    let coverage = darkhold_db::trope_clusters::coverage_for_work(state.db(), &id).await?;
    // `clusters` absent rather than empty is the §8.11 rule; here an instance with no clusters has genuinely measured nothing, so
    // the list is absent and says why.
    if coverage.is_empty() {
        return Ok(Json(serde_json::json!({
            "clusters": serde_json::Value::Null,
            "note": "no trope clusters are configured on this instance",
        })));
    }
    Ok(Json(serde_json::json!({
        "clusters": coverage,
        "named_count": coverage.iter().filter(|c| c.is_named()).count(),
        "total": coverage.len(),
    })))
}

/// Every cluster.
async fn list_clusters(
    State(state): State<AppState>,
    crate::auth::RequireSession(user): crate::auth::RequireSession,
) -> ApiResult<Json<serde_json::Value>> {
    crate::routes::experiment_sweep::require_operator(&state, &user)?;
    let clusters = darkhold_db::trope_clusters::all_clusters(state.db()).await?;
    Ok(Json(serde_json::json!({ "items": clusters })))
}

#[derive(Deserialize)]
struct CreateCluster {
    slug: String,
    label: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default = "default_priority")]
    priority: i64,
}

fn default_priority() -> i64 {
    100
}

/// Make one.
async fn create_cluster(
    State(state): State<AppState>,
    crate::auth::RequireSession(user): crate::auth::RequireSession,
    Json(body): Json<CreateCluster>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    crate::routes::experiment_sweep::require_operator(&state, &user)?;
    let cluster = darkhold_db::trope_clusters::create_cluster(
        state.db(),
        &body.slug,
        &body.label,
        body.description.as_deref(),
        body.priority,
    )
    .await
    .map_err(|error| match error.to_string().as_str() {
        message if message.contains("slug") || message.contains("label") => {
            ApiError(AppError::Validation {
                message: error.to_string(),
                field_errors: Default::default(),
            })
        }
        message if message.contains("UNIQUE") || message.contains("duplicate key") => {
            ApiError(AppError::Validation {
                message: "a cluster with that slug already exists".to_owned(),
                field_errors: Default::default(),
            })
        }
        _ => ApiError(AppError::Internal(error)),
    })?;
    Ok((StatusCode::CREATED, Json(serde_json::json!(cluster))))
}

/// Add a node to a cluster.
async fn add_node(
    State(state): State<AppState>,
    crate::auth::RequireSession(user): crate::auth::RequireSession,
    Path((slug, node_id)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    crate::routes::experiment_sweep::require_operator(&state, &user)?;
    darkhold_db::trope_clusters::add_node(state.db(), &slug, &node_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
