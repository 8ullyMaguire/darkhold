//! M70-05 — collection tags over HTTP.
//!
//! ```text
//! GET    /collections/:id/tags            what this collection is for
//! PUT    /collections/:id/tags/:node      attach one   (owner)
//! DELETE /collections/:id/tags/:node      detach one   (owner)
//! ```
//!
//! A collection tag is a claim about the collection, so writing one needs its owner. Reading one is as public as the
//! collection: the tag is part of how a reader decides whether the collection is for them.
//!
//! **These tags never reach a work.** Nothing in this module writes `work_tags`, and no work-facing query reads
//! `collection_tags` — that is the whole point of the table, and the db test asserts it rather than trusting the
//! comment.

use axum::extract::{Path, State};
use axum::routing::{get, put};
use axum::{Json, Router};
use serde_json::{json, Value};

use darkhold_domain::error::AppError;

use crate::auth::{MaybeSession, RequireSession};
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

/// The collection-tag routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/collections/{id}/tags", get(list))
        .route("/collections/{id}/tags/{node}", put(attach).delete(detach))
}

fn not_owned() -> ApiError {
    // The same answer for "not yours" and "not there": a distinct refusal would confirm a collection id.
    ApiError(AppError::Validation {
        message: "no collection with that id that you own".into(),
        field_errors: std::collections::BTreeMap::new(),
    })
}

async fn list(
    State(state): State<AppState>,
    MaybeSession(_session): MaybeSession,
    Path(collection): Path<String>,
) -> ApiResult<Json<Value>> {
    let tags = darkhold_db::collection_tags::list(state.db(), &collection).await?;
    Ok(Json(json!({ "items": tags, "count": tags.len() })))
}

async fn attach(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path((collection, node)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    if !darkhold_db::collection_tags::owned_by(
        state.db(),
        &collection,
        &user.account_id.to_string(),
    )
    .await?
    {
        return Err(not_owned());
    }
    let added = darkhold_db::collection_tags::add(state.db(), &collection, &node).await?;
    Ok(Json(json!({ "node_id": node, "added": added })))
}

async fn detach(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path((collection, node)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    if !darkhold_db::collection_tags::owned_by(
        state.db(),
        &collection,
        &user.account_id.to_string(),
    )
    .await?
    {
        return Err(not_owned());
    }
    let removed = darkhold_db::collection_tags::remove(state.db(), &collection, &node).await?;
    Ok(Json(json!({ "node_id": node, "removed": removed })))
}
