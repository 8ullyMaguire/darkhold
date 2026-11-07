//! Webhook subscription operations (M16-14): rotation, health, re-enable.
//!
//! ```text
//! POST /me/webhooks/{id}/rotate        owner: replace the signing secret, return the new one once
//! GET  /operator/webhooks              every subscription with its breaker state (no secrets)
//! POST /operator/webhooks/{id}/enable  operator: switch a self-disabled subscription back on
//! ```
//!
//! Three deliberate shapes:
//!
//! * The new secret is **returned once** and never readable again. A secret that can be fetched on demand
//!   is a secret that ends up in a dashboard, a log, or a support thread; rotation is the only read.
//! * The old secret stays usable for the grace window (`ROTATION_GRACE_SECONDS`), because a rotation whose
//!   effect is immediate breaks every subscriber mid-deploy — and the resulting failures look like an
//!   outage rather than a configuration change.
//! * Re-enabling is an operator act. The breaker switched the endpoint off because its receiver stopped
//!   answering; switching it back on is a judgement about the receiver, not an automatic retry.

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::http::{ApiError, ApiResult};
use crate::state::AppState;
use darkhold_domain::AppError;

/// The webhook operation routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/me/webhooks/{id}/rotate", post(rotate))
        .route("/operator/webhooks", get(health))
        .route("/operator/webhooks/{id}/enable", post(enable))
}

/// Replace a subscription's signing secret and return the new one — once.
///
/// The response says how long the old secret keeps working, because a subscriber that does not know the
/// window will deploy "immediately" and then report the grace as a bug.
async fn rotate(
    State(state): State<AppState>,
    crate::auth::RequireSession(user): crate::auth::RequireSession,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let owner = user.account_id.to_string();
    let secret = darkhold_domain::webhook::new_secret();
    let now = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();

    let rotated = darkhold_db::webhooks::rotate_secret(state.db(), &id, &owner, &secret, &now)
        .await
        .map_err(ApiError::from)?;

    match rotated {
        Some(secret) => Ok(Json(json!({
            "id": id,
            "secret": secret,
            "rotated_at": now,
            "previous_valid_for_seconds": darkhold_domain::webhook::ROTATION_GRACE_SECONDS,
            "note": "this is the only time this secret is returned; the replaced secret keeps verifying deliveries until the window ends",
        }))),
        // A missing subscription and somebody else's subscription are the same answer.
        None => Err(ApiError(AppError::field(
            "id",
            "no subscription with that id belongs to this account",
        ))),
    }
}

/// Every subscription with its breaker state.
///
/// The secret is absent by construction — an operator needs to know an endpoint is failing, not to be able
/// to forge its deliveries.
async fn health(
    State(state): State<AppState>,
    crate::auth::RequireSession(user): crate::auth::RequireSession,
) -> ApiResult<Json<Value>> {
    crate::routes::experiment_sweep::require_operator(&state, &user)?;
    let rows = darkhold_db::webhooks::subscriptions(state.db())
        .await
        .map_err(ApiError::from)?;
    Ok(Json(json!({ "subscriptions": rows })))
}

/// Switch a self-disabled subscription back on, resetting its breaker.
async fn enable(
    State(state): State<AppState>,
    crate::auth::RequireSession(user): crate::auth::RequireSession,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    crate::routes::experiment_sweep::require_operator(&state, &user)?;
    let enabled = darkhold_db::webhooks::reenable(state.db(), &id, None)
        .await
        .map_err(ApiError::from)?;
    if !enabled {
        return Err(ApiError(AppError::field(
            "id",
            "no subscription with that id",
        )));
    }
    Ok(Json(json!({ "id": id, "active": true })))
}
