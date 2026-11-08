//! M28 — Rating signal integrity: admin anomaly management and rating summary endpoints.
//!
//! ```text
//! GET  /admin/anomalies            list uncleared rating anomaly events
//! POST /admin/anomalies/:id/clear  clear an anomaly event
//! GET  /works/{id}/rating-summary  public trust-weighted rating summary
//! ```

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use darkhold_db::rating_integrity;
use darkhold_domain::ids::WorkId;
use serde_json::{json, Value};

use crate::auth::{MaybeSession, RequireSession};
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/anomalies", get(list_anomalies))
        .route("/admin/anomalies/{id}/clear", post(clear_anomaly))
        .route("/works/{id}/rating-summary", get(get_rating_summary))
        // M70-13: the queue's own listing, one work's events, the detection run, and the two contested doors §33.2 assigns
        // to a quorum of curators.
        .route("/works/{id}/anomalies", get(list_work_anomalies))
        .route("/admin/anomalies/detect", post(detect_anomalies))
        .route(
            "/admin/works/{id}/contested",
            post(contest_work).delete(uncontest_work),
        )
}

async fn list_anomalies(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<Value>> {
    // Declared operator in the inventory since the route was added, and enforced by nothing: any signed-in account could
    // read the anomaly list while the inventory promised an operator door. Found while classifying the route for ADR 0003 —
    // a declaration whose enforcement is missing is not a weaker gate, it is no gate.
    crate::routes::admin_scope::require_admin_of(&state, &user, Some("darkhold")).await?;

    // This used to answer `{count: n, items: []}`: it counted the queue and listed nothing, so an operator could see
    // that seven anomalies existed and never learn which works they belonged to.
    let events = rating_integrity::list_uncleared_anomalies(state.db(), 200)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;
    let items: Vec<Value> = events.iter().map(event_view).collect();
    Ok(Json(json!({ "count": items.len(), "items": items })))
}

/// One work's uncleared anomaly events, for the work's own moderation page.
///
/// The per-work read is the one the schema was built for (`get_work_anomaly_events`), and the queue above is the same
/// rows across works. Both exist because they answer different questions: "what is wrong with this work" and "what needs
/// attention now".
async fn list_work_anomalies(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    crate::routes::admin_scope::require_admin_of(&state, &user, Some("darkhold")).await?;
    let work_id = parse_work_id(&id)?;
    let events = rating_integrity::get_work_anomaly_events(state.db(), &work_id)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;
    let items: Vec<Value> = events.iter().map(event_view).collect();
    Ok(Json(json!({ "work_id": work_id.to_string(), "items": items })))
}

/// Run burst detection once.
///
/// The producer `insert_rating_anomaly_event` never had. It records events and marks nothing contested: §33.2 gives the
/// contested mark to a quorum of curators, and a detector that could hide a work's aggregate on its own would be a
/// machine moderator.
async fn detect_anomalies(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<Value>> {
    crate::routes::admin_scope::require_admin_of(&state, &user, Some("darkhold")).await?;
    let report = crate::rating_watch::detect_bursts(&state)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;
    Ok(Json(json!(report)))
}

/// Mark a work contested: its aggregate stops being published as a public score.
///
/// The reason is required. A contested work publishes no number, so the reason is the *only* explanation a curator — or
/// the author, in a support thread — will ever get, and "contested: (nothing)" is indistinguishable from a bug.
async fn contest_work(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult<Json<Value>> {
    crate::routes::admin_scope::require_admin_of(&state, &user, Some("darkhold")).await?;
    let reason = body
        .get("reason")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|reason| !reason.is_empty())
        .ok_or_else(|| {
            let mut field_errors = std::collections::BTreeMap::new();
            field_errors.insert(
                "reason".to_string(),
                "why the aggregate is withdrawn, for the curator who clears it".to_string(),
            );
            ApiError(darkhold_domain::AppError::Validation {
                message: "a contested mark needs a reason".to_string(),
                field_errors,
            })
        })?;
    let work_id = parse_work_id(&id)?;
    rating_integrity::set_work_contested(state.db(), &work_id, reason)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;
    Ok(Json(json!({ "work_id": work_id.to_string(), "contested": true, "reason": reason })))
}

/// Clear a work's contested mark.
async fn uncontest_work(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    crate::routes::admin_scope::require_admin_of(&state, &user, Some("darkhold")).await?;
    let work_id = parse_work_id(&id)?;
    rating_integrity::clear_work_contested(state.db(), &work_id)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;
    Ok(Json(json!({ "work_id": work_id.to_string(), "contested": false })))
}

/// The JSON shape one anomaly event has in both listings.
fn event_view(event: &rating_integrity::RatingAnomalyEvent) -> Value {
    json!({
        "id": event.id,
        "work_id": event.work_id,
        "cohort_id": event.cohort_id,
        "kind": event.kind,
        "severity": event.severity,
        "detail": serde_json::from_str::<Value>(&event.detail).unwrap_or_else(|_| Value::String(event.detail.clone())),
        "detected_at": event.detected_at,
    })
}

fn parse_work_id(id: &str) -> ApiResult<WorkId> {
    id.parse::<WorkId>().map_err(|_| {
        ApiError(darkhold_domain::AppError::Validation {
            message: "invalid work id".to_string(),
            field_errors: Default::default(),
        })
    })
}

async fn clear_anomaly(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    // Same gap as `list_anomalies`, and worse: clearing an anomaly erases the escalation record that a future moderator
    // would have read.
    crate::routes::admin_scope::require_admin_of(&state, &user, Some("darkhold")).await?;
    let db = state.db();
    let clearer = user.account_id;
    rating_integrity::clear_rating_anomaly_event(db, &id, &clearer)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;
    Ok(Json(json!({ "id": id, "cleared": true })))
}

async fn get_rating_summary(
    State(state): State<AppState>,
    MaybeSession(_session): MaybeSession,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let db = state.db();
    let work_id: WorkId = id.parse().map_err(|_| {
        ApiError(darkhold_domain::AppError::Validation {
            message: "invalid work id".to_string(),
            field_errors: Default::default(),
        })
    })?;
    // §33.2: a contested work's aggregate is "visible to curators, never published as a public score", and an aggregate
    // display "states when a cohort guard is suppressing a number". So the door answers with the reason rather than a
    // number: a silent `null` would read as "nobody has rated this", which is a different and untrue claim.
    let contested = rating_integrity::is_work_contested(db, &work_id)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;
    if contested {
        return Ok(Json(json!({
            "count": Value::Null,
            "mean_stars": Value::Null,
            "suppressed": "contested",
            "method": "trust-weighted mean of public ratings",
        })));
    }

    let summary = rating_integrity::get_trust_weighted_rating_summary(db, &work_id)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;
    Ok(Json(match summary {
        Some(s) => json!({
            "count": s.count,
            "mean_stars": s.mean_permille as f64 / 1000.0,
            "method": "trust-weighted mean of public ratings",
        }),
        None => json!({ "count": 0, "mean_stars": null, "method": null }),
    }))
}
