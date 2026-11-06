//! Tag scripts: a kept set of assertions, applied while browsing (M69-20).
//!
//! ```text
//! GET/POST               /tag-scripts
//! POST/DELETE            /tag-scripts/:id/nodes/:nodeId
//! POST                   /tag-scripts/:id/apply/:workId
//! ```
//!
//! The apply route is the point of the feature: gardening is a **mode**, so the gesture while reading is one call and the work page
//! refreshes with the tags on it. The write goes through `tag_assertions`, which is what makes a script's tags carry a human
//! claimant rather than being a second kind of tag.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use crate::http::{ApiError, ApiResult};
use crate::state::AppState;
use darkhold_domain::AppError;

/// The tag-script routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/tag-scripts", get(list_scripts).post(create_script))
        .route(
            "/tag-scripts/{id}/nodes/{node_id}",
            post(add_node).delete(remove_node),
        )
        .route("/tag-scripts/{id}/apply/{work_id}", post(apply))
}

#[derive(Deserialize)]
struct CreateScript {
    name: String,
}

/// The caller's scripts.
async fn list_scripts(
    State(state): State<AppState>,
    crate::auth::RequireSession(user): crate::auth::RequireSession,
) -> ApiResult<Json<serde_json::Value>> {
    let scripts =
        darkhold_db::tag_scripts::scripts_for(state.db(), &user.account_id.to_string()).await?;
    Ok(Json(serde_json::json!({ "items": scripts })))
}

/// Make one.
async fn create_script(
    State(state): State<AppState>,
    crate::auth::RequireSession(user): crate::auth::RequireSession,
    Json(body): Json<CreateScript>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    let script = darkhold_db::tag_scripts::create_script(
        state.db(),
        &user.account_id.to_string(),
        &body.name,
    )
    .await
    .map_err(|error| match error.to_string().as_str() {
        message if message.contains("UNIQUE") || message.contains("duplicate key") => {
            ApiError(AppError::Validation {
                message: "you already have a tag script with that name".to_owned(),
                field_errors: Default::default(),
            })
        }
        _ => ApiError(AppError::Internal(error)),
    })?;
    Ok((StatusCode::CREATED, Json(serde_json::json!(script))))
}

/// Add a tag to a script.
async fn add_node(
    State(state): State<AppState>,
    crate::auth::RequireSession(user): crate::auth::RequireSession,
    Path((id, node_id)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    let changed = darkhold_db::tag_scripts::add_node(
        state.db(),
        &user.account_id.to_string(),
        &id,
        &node_id,
    )
    .await?;
    // `false` means either "not your script" or "already in it". Both answer the same way on purpose: the second is a no-op the
    // caller asked for, and the first must not be distinguishable from it or the endpoint becomes a probe for other scripts.
    let _ = changed;
    Ok(StatusCode::NO_CONTENT)
}

/// Remove a tag from a script.
async fn remove_node(
    State(state): State<AppState>,
    crate::auth::RequireSession(user): crate::auth::RequireSession,
    Path((id, node_id)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    darkhold_db::tag_scripts::remove_node(
        state.db(),
        &user.account_id.to_string(),
        &id,
        &node_id,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Apply a script to a work — the browsing gesture.
async fn apply(
    State(state): State<AppState>,
    crate::auth::RequireSession(user): crate::auth::RequireSession,
    Path((id, work_id)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let report = darkhold_db::tag_scripts::apply_to_work(
        state.db(),
        &user.account_id.to_string(),
        &id,
        &work_id,
    )
    .await?
    .ok_or_else(|| ApiError(AppError::NotFound {
        resource: "tag script",
    }))?;
    Ok(Json(serde_json::json!(report)))
}
