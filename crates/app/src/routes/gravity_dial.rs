//! `GET/PUT/DELETE /api/v1/discovery/gravity` — the reader's own dial (M64-09, spec §16.5).

use crate::auth::{RequireSession, SessionUser};
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use darkhold_domain::error::AppError;
use darkhold_domain::gravity_dial::{effective, narrow, DialRefusal};
use serde::Deserialize;
use serde_json::{json, Value};

/// The surface whose dial this is. One today; named rather than inlined so a second dial cannot quietly share a row.
const SURFACE: &str = "discover.feed";

#[derive(Deserialize)]
struct DialRequest {
    strength: f64,
}

/// A dial belongs to a pseud, not to an account: the row is keyed by the pseud so two personas can hold two answers. An
/// account that has not chosen one gets a field error naming the missing part rather than a foreign-key failure.
fn require_pseud(session: &SessionUser) -> ApiResult<uuid::Uuid> {
    session
        .pseud_id
        .as_ref()
        .map(|pseud| pseud.as_uuid())
        .ok_or_else(|| {
            ApiError(AppError::field(
                "pseud",
                "choose a pseud before setting gravity",
            ))
        })
}

pub fn router() -> Router<AppState> {
    // Paths here are relative to the `/api/v1` nest the authenticated group is mounted under, which is how the inventory
    // table records every API route.
    Router::new().route("/discovery/gravity", get(read).put(write).delete(reset))
}

async fn read(State(state): State<AppState>, session: RequireSession) -> ApiResult<Json<Value>> {
    let pseud = require_pseud(&session.0)?;
    let taste = &state.config().taste;
    let instance = taste.strength_for(SURFACE);
    let stored = darkhold_db::gravity_prefs::read(state.db(), pseud)
        .await
        .map_err(|error| ApiError(AppError::internal("reading the gravity dial", error)))?;

    Ok(Json(json!({
        "surface": SURFACE,
        // Three numbers, because the reader is owed the difference between what the instance runs, what they asked for, and
        // what they are getting: a dial that only reported the third would hide the ceiling it is subject to.
        "instance": instance,
        "yours": stored,
        "effective": effective(instance, stored),
        "locked": taste.gravity_locked,
        "ceiling": taste.gravity_ceiling,
    })))
}

async fn write(
    State(state): State<AppState>,
    session: RequireSession,
    Json(body): Json<DialRequest>,
) -> ApiResult<Json<Value>> {
    let pseud = require_pseud(&session.0)?;
    let taste = &state.config().taste;
    let instance = taste.strength_for(SURFACE);

    let strength = narrow(instance, body.strength, taste.gravity_locked).map_err(|refusal| {
        // Each refusal says what would have been accepted: "between 0 and 0.15" is actionable, "invalid" is not.
        match refusal {
            DialRefusal::Locked => ApiError(AppError::field(
                "strength",
                "gravity is locked on this instance; the operator set it and readers cannot change it",
            )),
            DialRefusal::OutOfRange => ApiError(AppError::field(
                "strength",
                "must be between 0.0 and 1.0 inclusive",
            )),
            DialRefusal::AboveInstance(instance) => ApiError(AppError::field(
                "strength",
                format!(
                    "the dial narrows, it never widens: this instance runs {instance}, so ask for at most that"
                ),
            )),
        }
    })?;

    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    darkhold_db::gravity_prefs::set(state.db(), pseud, strength, &now)
        .await
        .map_err(|error| ApiError(AppError::internal("storing the gravity dial", error)))?;

    Ok(Json(json!({
        "surface": SURFACE,
        "instance": instance,
        "yours": strength,
        "effective": effective(instance, Some(strength)),
    })))
}

async fn reset(State(state): State<AppState>, session: RequireSession) -> ApiResult<Json<Value>> {
    let pseud = require_pseud(&session.0)?;
    let cleared = darkhold_db::gravity_prefs::clear(state.db(), pseud)
        .await
        .map_err(|error| ApiError(AppError::internal("clearing the gravity dial", error)))?;
    let instance = state.config().taste.strength_for(SURFACE);
    Ok(Json(json!({
        "surface": SURFACE,
        "cleared": cleared,
        "instance": instance,
        "effective": instance,
    })))
}
