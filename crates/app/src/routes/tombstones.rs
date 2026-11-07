//! M69-04 — the tombstone doors: file a takedown, read the list, withdraw one.
//!
//! * `POST /api/v1/admin/tombstones` — file by work identity (title, author, language as the importer sees them).
//! * `GET  /api/v1/admin/tombstones` — every tombstone, withdrawn ones included, with how many imports each has stopped.
//! * `POST /api/v1/admin/tombstones/{id}/withdraw` — undo an over-broad filing, with a reason.
//!
//! All three are operator-only and closed unless `[discovery] work_tombstones` is on.
//!
//! # Why filing takes title + author, not a work id
//!
//! The failure this exists to stop is a work that is **not here yet** — it is on a mirror, or it will be tomorrow. A work id
//! would only let an operator tombstone something already imported, which is the case that does not need help. So the door
//! takes the identity the importer computes, and says so: the response echoes the key it derived, because an operator who
//! files a tombstone that never matches deserves to see the key they actually filed.
//!
//! # Why the list shows refusals
//!
//! A tombstone with `refusals: 0` is a guess — usually a title spelled differently from the source's, or an author the
//! source does not carry. Without the count an operator cannot tell a working takedown from a typo.

use axum::extract::{Path, State};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::RequireSession;
use crate::http::{ApiError, ApiResult};
use crate::routes::discovery::require_operator;
use crate::state::AppState;
use darkhold_domain::error::AppError;

/// The tombstone routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/tombstones", post(file).get(list))
        .route("/admin/tombstones/{id}/withdraw", post(withdraw))
}

fn closed(state: &AppState) -> Option<ApiError> {
    (!state.config().discovery.work_tombstones).then(|| {
        ApiError(AppError::NotFound {
            resource: "tombstones",
        })
    })
}

fn internal(error: anyhow::Error) -> ApiError {
    ApiError(AppError::Internal(error))
}

#[derive(Debug, Deserialize)]
struct FileRequest {
    /// The work's title, as an importer would see it.
    title: String,
    /// The author as displayed. Empty is a real value: it is not a wildcard.
    #[serde(default)]
    author: String,
    #[serde(default)]
    language: Option<String>,
    /// Why: `author request`, `dmca`, `court order`, ...
    reason: String,
    /// Where the work was seen. Not part of the key.
    #[serde(default)]
    origin_url: Option<String>,
    /// The DMCA paperwork's id, when there is any.
    #[serde(default)]
    dmca_id: Option<String>,
    #[serde(default)]
    note: String,
}

async fn file(
    State(state): State<AppState>,
    session: RequireSession,
    Json(request): Json<FileRequest>,
) -> ApiResult<Json<Value>> {
    if let Some(error) = closed(&state) {
        return Err(error);
    }
    require_operator(&state, &session.0)?;
    if request.reason.trim().is_empty() {
        return Err(ApiError(AppError::Validation {
            message: "a tombstone needs a reason: it is what the refusal report cites".to_owned(),
            field_errors: std::collections::BTreeMap::new(),
        }));
    }

    let now = darkhold_db::identity::now_rfc3339();
    let filed_by = session.0.account_id.to_string();
    let tombstone = darkhold_db::tombstones::file(
        state.db(),
        &darkhold_db::tombstones::Filing {
            title: &request.title,
            author: &request.author,
            language: request.language.as_deref(),
            reason: request.reason.trim(),
            origin_url: request.origin_url.as_deref(),
            dmca_id: request.dmca_id.as_deref(),
            filed_by: &filed_by,
            note: &request.note,
            now: &now,
        },
    )
    .await
    .map_err(|error| {
        ApiError(AppError::Validation {
            message: error.to_string(),
            field_errors: std::collections::BTreeMap::new(),
        })
    })?;

    Ok(Json(json!({
        "id": tombstone.id,
        "identity": tombstone.identity,
        "title": tombstone.title,
        "author": tombstone.author,
        "reason": tombstone.reason,
        "filed_at": tombstone.filed_at,
        "refusals": tombstone.refusals,
        "note": "the identity key is what the importer computes; a tombstone whose key never matches is visible as a zero in `refusals`",
    })))
}

async fn list(
    State(state): State<AppState>,
    session: RequireSession,
) -> ApiResult<Json<Value>> {
    if let Some(error) = closed(&state) {
        return Err(error);
    }
    require_operator(&state, &session.0)?;
    let rows = darkhold_db::tombstones::list(state.db(), 200)
        .await
        .map_err(internal)?;

    Ok(Json(json!({
        "tombstones": rows.iter().map(|row| json!({
            "id": row.id,
            "identity": row.identity,
            "title": row.title,
            "author": row.author,
            "language": row.language,
            "reason": row.reason,
            "origin_url": row.origin_url,
            "dmca_id": row.dmca_id,
            "filed_by": row.filed_by,
            "filed_at": row.filed_at,
            "note": row.note,
            "refusals": row.refusals,
            "last_refused_at": row.last_refused_at,
            "withdrawn_at": row.withdrawn_at,
            "withdraw_reason": row.withdraw_reason,
        })).collect::<Vec<Value>>(),
        "active": rows.iter().filter(|row| row.withdrawn_at.is_none()).count(),
        "note": "withdrawn tombstones stay in the list: the row is the answer to 'why did my work vanish?'",
    })))
}

#[derive(Debug, Deserialize)]
struct WithdrawRequest {
    reason: String,
}

async fn withdraw(
    State(state): State<AppState>,
    session: RequireSession,
    Path(id): Path<String>,
    Json(request): Json<WithdrawRequest>,
) -> ApiResult<Json<Value>> {
    if let Some(error) = closed(&state) {
        return Err(error);
    }
    require_operator(&state, &session.0)?;
    let now = darkhold_db::identity::now_rfc3339();
    let removed = darkhold_db::tombstones::withdraw(
        state.db(),
        &id,
        &session.0.account_id.to_string(),
        request.reason.trim(),
        &now,
    )
    .await
    .map_err(|error| {
        ApiError(AppError::Validation {
            message: error.to_string(),
            field_errors: std::collections::BTreeMap::new(),
        })
    })?;
    if !removed {
        return Err(ApiError(AppError::NotFound {
            resource: "active tombstone",
        }));
    }
    Ok(Json(json!({
        "withdrawn": id,
        "at": now,
        "note": "the tombstone stops refusing and stays in the list, with the reason recorded",
    })))
}
