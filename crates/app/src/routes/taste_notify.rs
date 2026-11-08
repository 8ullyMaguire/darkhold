//! The taste-weighted notification queue over HTTP (M70-13, migration 0055).
//!
//! The producer runs inside the publish path (`crate::taste_notify::queue_aligned_readers`). What needed a caller here is
//! the other half: nothing drained `taste_notification_queue`, so a queue the instance had no way to fill *and* no way to
//! empty would have been a backlog nobody could see.
//!
//! Two doors, and the refusal that belongs to each:
//!
//! * **`POST /admin/taste-notifications/drain`** — the operator's. A drain is a send: it writes an inbox row per queued
//!   reader, so it is not a route a reader can trigger. `limit` is clamped because each row is a write to somebody else's
//!   inbox, not a page of results.
//! * **`GET /me/taste-notifications`** — the reader's. Their own pending count, so a queue building up behind an
//!   undrained worker is visible to the person it is for rather than only to the operator. It is a count and not the rows:
//!   a queued row is a *plan* to notify, and showing the plan would deliver the notification without marking it sent.

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::RequireSession;
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/taste-notifications/drain", post(drain))
        .route("/me/taste-notifications", get(my_pending))
}

#[derive(Debug, Deserialize)]
pub struct DrainBody {
    /// How many queued notifications to deliver. Each one is an inbox row for a real reader, so the ceiling is low.
    #[serde(default)]
    pub limit: Option<i64>,
}

/// Deliver the queued taste notifications. Operator only: it writes to other people's inboxes.
async fn drain(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Json(body): Json<DrainBody>,
) -> ApiResult<Json<Value>> {
    crate::routes::admin_scope::require_estate_hidden(&state, &user)?;
    let limit = body.limit.unwrap_or(50).clamp(1, 200);
    let outcome = crate::taste_notify::drain_and_deliver(&state, limit)
        .await
        .map_err(|error| ApiError(darkhold_domain::AppError::Internal(error)))?;
    Ok(Json(json!({
        "drained": outcome.drained,
        "delivered": outcome.delivered,
        "failed": outcome.failed,
        // Said out loud, because "delivered" counts rows written and the reader's own settings decide whether a row means
        // anything (`notifications::notify` drops a disabled kind and still returns an id).
        "note": "delivered counts inbox rows written; a reader who turned taste suggestions off receives none",
        "limit": limit,
    })))
}

/// The caller's own pending count, and the cap the producer enforces.
async fn my_pending(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<Value>> {
    let pending = darkhold_db::engagement::pending_taste_notification_count(
        state.db(),
        &user.account_id.to_string(),
    )
    .await
    .map_err(|error| ApiError(darkhold_domain::AppError::Internal(error.into())))?;
    Ok(Json(json!({
        "pending": pending,
        "cap": crate::taste_notify::PENDING_CAP,
        "module_enabled": state.modules().enabled("taste_notifications"),
    })))
}
