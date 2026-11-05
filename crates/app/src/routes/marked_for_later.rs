//! M70-07 — Marked for Later over HTTP.
//!
//! ```text
//! POST   /works/:id/marked-for-later    queue it (idempotent)
//! DELETE /works/:id/marked-for-later    drop it
//! GET    /me/marked-for-later           the queue, newest first
//! ```
//!
//! # Why there is no `GET /works/:id/marked-for-later`
//!
//! Asking "is this work in *my* queue" is answered by `WORK_STATE` on the work page, and a per-work read would
//! invite a caller to ask about somebody else's queue with the same URL shape. The only queue this door returns is
//! the caller's own, which is why the read is `/me/...`.
//!
//! # Why it is not under `/bookmarks`
//!
//! A bookmark is a record and this is a queue: the entry goes when the work is finished, it never counts, and it
//! feeds no taste signal. Mounting it beside bookmarks would have made those differences a filter somebody has to
//! remember instead of a property of the table.

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use darkhold_db::content;
use darkhold_domain::error::AppError;

use crate::auth::RequireSession;
use crate::http::{ApiError, ApiResult};
use crate::routes::works::parse_work_id;
use crate::state::AppState;

/// The queue routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/works/{id}/marked-for-later", post(mark).delete(unmark))
        .route("/me/marked-for-later", get(queue))
}

#[derive(Debug, Deserialize)]
struct QueueQuery {
    limit: Option<i64>,
}

async fn mark(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(work): Path<String>,
) -> ApiResult<Json<Value>> {
    let work_id = parse_work_id(&work)?;
    // A queue entry for a work that does not exist would be a row pointing at nothing; the FK would refuse it with
    // a 500, which reads as our fault rather than as the wrong id.
    content::find_work(state.db(), work_id)
        .await?
        .ok_or_else(|| ApiError(AppError::NotFound { resource: "work" }))?;
    let added =
        darkhold_db::marked_for_later::mark(state.db(), &user.account_id.to_string(), &work)
            .await?;
    Ok(Json(json!({ "marked": true, "added": added })))
}

async fn unmark(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(work): Path<String>,
) -> ApiResult<Json<Value>> {
    let removed =
        darkhold_db::marked_for_later::unmark(state.db(), &user.account_id.to_string(), &work)
            .await?;
    Ok(Json(json!({ "marked": false, "removed": removed })))
}

async fn queue(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Query(query): Query<QueueQuery>,
) -> ApiResult<Json<Value>> {
    let limit = query.limit.unwrap_or(100).clamp(1, 200);
    let items =
        darkhold_db::marked_for_later::queue(state.db(), &user.account_id.to_string(), limit)
            .await?;
    Ok(Json(json!({ "items": items, "count": items.len() })))
}
