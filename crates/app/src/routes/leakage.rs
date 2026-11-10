//! The batch-window and resonance doors — the operator surfaces §52.2 and §52.3 need (M70-13).
//!
//! ## Why this file exists
//!
//! Migration 0110 enforces the leakage rules in the schema, which is where they should be enforced: a rule kept in two
//! places drifts, and the schema cannot be argued with. What the schema cannot say is that a payout belongs to a *batch
//! window*, and that a resonance label is stale when the window it was computed in has not closed. `taste_leakage.rs`
//! says that, in eight functions that — until this file — had no caller and no route anywhere in `crates/app`.
//!
//! Six of the eight get their door here. `record_payout` and `get_payout` do not: a payout row is written by a payout
//! *run*, and there is no run yet. A `GET /admin/leakage/payouts/{id}` nobody can populate would be the same dead
//! surface in the other direction, so they are named in the ledger against the payouts row rather than dressed up here.
//!
//! ## The shape of the window
//!
//! §52.2's promise is that reading someone's taste cannot be correlated with a payout *because the payout lands after
//! the window closes and covers many readers*. That promise is only kept if the window is a real object with an open
//! and a close, which is what these doors make it. Opening while a window is already open is refused rather than
//! silently reused: two windows overlapping would mean a payout attributable to either, which is the thing the rule
//! exists to prevent.
//!
//! Timestamps cross the wire as unix seconds, because that is what the domain holds (`UnixSeconds`) and converting to
//! RFC 3339 here would add a second place to disagree about the offset.

use axum::extract::{Path, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde_json::{json, Value};

use darkhold_domain::leakage::{BatchWindow, OwnerResonance};
use darkhold_domain::{AppError, WorkId};

use crate::auth::RequireSession;
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/admin/leakage/window",
            get(latest_window).post(open_window),
        )
        .route("/admin/leakage/window/{id}/close", post(close_window))
        .route("/admin/works/{id}/resonance", put(set_resonance))
        .route("/works/{id}/resonance", get(read_resonance))
}

fn internal(error: impl std::fmt::Display) -> ApiError {
    ApiError(AppError::Internal(anyhow::anyhow!("{error}")))
}

fn now_seconds() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

fn window_view(id: &uuid::Uuid, window: &BatchWindow) -> Value {
    json!({
        "id": id.to_string(),
        "opened_at": window.opened_at,
        "closed_at": window.closed_at,
        "open": window.closed_at.is_none(),
    })
}

/// The newest window, open or closed, or `null`.
///
/// The operator question this answers is "which window is live, and do I owe a close", so an empty answer is `null`
/// rather than an error: a fresh instance has never batched, and that is a state, not a fault.
async fn latest_window(
    State(state): State<AppState>,
    session: RequireSession,
) -> ApiResult<Json<Value>> {
    crate::routes::discovery::require_operator(&state, &session.0)?;
    let latest = darkhold_db::taste_leakage::latest_window(state.db())
        .await
        .map_err(internal)?;
    Ok(Json(match latest {
        None => json!({ "window": Value::Null }),
        Some((id, window)) => json!({ "window": window_view(&id, &window) }),
    }))
}

async fn open_window(
    State(state): State<AppState>,
    session: RequireSession,
    body: Option<Json<Value>>,
) -> ApiResult<Json<Value>> {
    crate::routes::discovery::require_operator(&state, &session.0)?;

    // One window at a time. Opening over an open window would make a payout attributable to two batches, which is
    // exactly the correlation §52.2 forbids — so the refusal is the feature.
    if let Some((id, window)) = darkhold_db::taste_leakage::latest_window(state.db())
        .await
        .map_err(internal)?
    {
        if window.closed_at.is_none() {
            let mut field_errors = std::collections::BTreeMap::new();
            field_errors.insert(
                "window".to_string(),
                format!("window {id} opened at {} is still open", window.opened_at),
            );
            return Err(ApiError(AppError::Validation {
                message: "a batch window is already open".to_string(),
                field_errors,
            }));
        }
    }

    let opened_at = body
        .as_ref()
        .and_then(|Json(value)| value["opened_at"].as_i64())
        .unwrap_or_else(now_seconds);
    let id = darkhold_db::taste_leakage::open_window(state.db(), opened_at)
        .await
        .map_err(internal)?;
    let window = darkhold_db::taste_leakage::get_window(state.db(), id)
        .await
        .map_err(internal)?
        .ok_or_else(|| {
            ApiError(AppError::Internal(anyhow::anyhow!(
                "a window that was just inserted is missing"
            )))
        })?;
    Ok(Json(window_view(&id, &window)))
}

/// Close a window, which is the moment its payouts become attributable to a batch rather than to a reader.
///
/// Idempotent in the safe direction: closing an already-closed window is refused rather than re-stamped, because a
/// second `closed_at` would move the boundary the labels are judged stale against.
async fn close_window(
    State(state): State<AppState>,
    session: RequireSession,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    crate::routes::discovery::require_operator(&state, &session.0)?;
    let window_id: uuid::Uuid = id.parse().map_err(|_| {
        ApiError(AppError::NotFound {
            resource: "batch window",
        })
    })?;
    let window = darkhold_db::taste_leakage::get_window(state.db(), window_id)
        .await
        .map_err(internal)?
        .ok_or_else(|| {
            ApiError(AppError::NotFound {
                resource: "batch window",
            })
        })?;
    if window.closed_at.is_some() {
        let mut field_errors = std::collections::BTreeMap::new();
        field_errors.insert("window".to_string(), "already closed".to_string());
        return Err(ApiError(AppError::Validation {
            message: "that batch window is already closed".to_string(),
            field_errors,
        }));
    }
    let closed_at = now_seconds().max(window.opened_at);
    darkhold_db::taste_leakage::close_window(state.db(), window_id, closed_at)
        .await
        .map_err(internal)?;
    Ok(Json(window_view(
        &window_id,
        &BatchWindow::closed(window.opened_at, closed_at),
    )))
}

/// A work's resonance label, to its owner or to an operator.
///
/// The label is a bucket, never a score (§52.3): the owner learns "noticed", not the number behind it, so this door
/// cannot be used to reverse-engineer how many readers were counted.
async fn read_resonance(
    State(state): State<AppState>,
    session: RequireSession,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let account = session.0.account_id.to_string();
    if crate::routes::discovery::require_operator(&state, &session.0).is_err() {
        let owned = darkhold_db::orphan::owned_by(state.db(), &id, &account)
            .await
            .map_err(internal)?;
        if !owned {
            return Err(ApiError(AppError::NotFound { resource: "work" }));
        }
    }
    let work: WorkId = id
        .parse()
        .map_err(|_| ApiError(AppError::NotFound { resource: "work" }))?;
    let label = darkhold_db::taste_leakage::get_resonance_label(state.db(), work)
        .await
        .map_err(internal)?;
    Ok(Json(match label {
        None => json!({ "work_id": id, "label": Value::Null }),
        Some((label, computed_at, window)) => json!({
            "work_id": id,
            "label": label.as_str(),
            "computed_at": computed_at,
            "window_id": window.to_string(),
        }),
    }))
}

/// Set a work's label, for an operator — the write half of a batch that has already closed.
///
/// The owner is read from the work rather than taken from the caller: a body claiming `owner_pseud_id` would let one
/// operator's mistake, or one client's bug, write a label against the wrong author, and the label is what pays out.
async fn set_resonance(
    State(state): State<AppState>,
    session: RequireSession,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult<Json<Value>> {
    crate::routes::discovery::require_operator(&state, &session.0)?;

    let work: WorkId = id
        .parse()
        .map_err(|_| ApiError(AppError::NotFound { resource: "work" }))?;
    let label_text = body["label"].as_str().unwrap_or_default();
    let Some(label) = OwnerResonance::parse(label_text) else {
        let mut field_errors = std::collections::BTreeMap::new();
        field_errors.insert(
            "label".to_string(),
            "one of quiet, steady, noticed, landing".to_string(),
        );
        return Err(ApiError(AppError::Validation {
            message: format!("`{label_text}` is not a resonance label"),
            field_errors,
        }));
    };

    let owner = darkhold_db::collaboration::owner_of(state.db(), work)
        .await
        .map_err(internal)?
        .ok_or_else(|| ApiError(AppError::NotFound { resource: "work" }))?;

    // The window comes from the caller when given (a backfill of an old batch) and from the latest window otherwise —
    // but a window that is still open cannot carry a label: its labels would be stale the moment it closed.
    let window_id = match body["window_id"].as_str() {
        Some(raw) => raw.parse::<uuid::Uuid>().map_err(|_| {
            ApiError(AppError::NotFound {
                resource: "batch window",
            })
        })?,
        None => match darkhold_db::taste_leakage::latest_window(state.db())
            .await
            .map_err(internal)?
        {
            Some((id, window)) if window.closed_at.is_some() => id,
            Some(_) => {
                let mut field_errors = std::collections::BTreeMap::new();
                field_errors.insert(
                    "window".to_string(),
                    "the open window has not closed".to_string(),
                );
                return Err(ApiError(AppError::Validation {
                    message: "a label needs a closed batch window".to_string(),
                    field_errors,
                }));
            }
            None => {
                let mut field_errors = std::collections::BTreeMap::new();
                field_errors.insert("window".to_string(), "no batch window exists".to_string());
                return Err(ApiError(AppError::Validation {
                    message: "a label needs a closed batch window".to_string(),
                    field_errors,
                }));
            }
        },
    };

    let computed_at = now_seconds();
    darkhold_db::taste_leakage::set_resonance_label(
        state.db(),
        owner,
        work,
        label,
        window_id,
        computed_at,
    )
    .await
    .map_err(internal)?;
    Ok(Json(json!({
        "work_id": id,
        "label": label.as_str(),
        "computed_at": computed_at,
        "window_id": window_id.to_string(),
    })))
}
