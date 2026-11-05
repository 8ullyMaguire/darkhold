//! W5 task 7 — the operator's fixity view, at `/api/v1/admin/fixity`.
//!
//! A drift row names a storage key, and a storage key says what this instance stores. The route therefore
//! answers **404** to anyone who is not the operator, never 403: "yes, and you may not" is itself a
//! disclosure, which is the rule `admin_discovery::require_operator` states and this module repeats rather than
//! imports — the two views must not become one endpoint the day somebody widens one of them.

use axum::extract::State;
use axum::routing::{get, post};
use axum::Json;
use serde_json::{json, Value};

use crate::auth::{RequireSession, SessionUser};
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

use darkhold_domain::error::AppError;

/// Refuse a caller who is not this instance's operator, without confirming that the view exists.
fn require_operator(state: &AppState, user: &SessionUser) -> ApiResult<()> {
    if state.config().administration.operator_account_id == Some(user.account_id) {
        return Ok(());
    }
    tracing::debug!(
        operator_configured = state.config().administration.operator_account_id.is_some(),
        "the fixity view was reached by an account that is not the operator"
    );
    Err(ApiError(AppError::NotFound {
        resource: "fixity report",
    }))
}

/// How many bytes one interactive run may hash.
///
/// An HTTP caller must not be able to make the instance read forty gigabytes. Two gigabytes is a promise about
/// wall-clock; a pass that stops early says `complete: false` rather than pretending it finished, so a budget
/// that is too small is visible instead of silent.
const RUN_BUDGET_BYTES: i64 = 2 * 1024 * 1024 * 1024;

/// `GET /api/v1/admin/fixity/report` — the last run and what it found.
///
/// # Errors
/// 404 for a non-operator; 500 if the report cannot be read.
pub async fn get_report(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<Value>> {
    require_operator(&state, &user)?;
    let report = crate::fixity::latest_report(state.db())
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
    Ok(Json(json!({
        "report": report,
        // The report never certifies that the archive is sound. A clean run says the bytes it read were the
        // bytes it expected; it says nothing about the blobs a budget kept it from opening.
        "note": "A clean run covers the blobs it checked. A run with complete=false stopped at its budget.",
    })))
}

/// `POST /api/v1/admin/fixity/run` — scan now, bounded.
///
/// # Errors
/// 404 for a non-operator; 500 if the run's own rows cannot be written.
pub async fn post_run(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<Value>> {
    require_operator(&state, &user)?;
    let store = darkhold_db::storage::BlobStore::new(state.config().storage.root.clone());
    let summary = crate::fixity::scan(state.db(), &store, RUN_BUDGET_BYTES)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
    Ok(Json(json!({
        "run_id": summary.run_id,
        "clean": summary.clean,
        "drifted": summary.drifted,
        "missing": summary.missing,
        "unreadable": summary.unreadable,
        "bytes_hashed": summary.bytes_hashed,
        "complete": summary.complete,
        "elapsed_ms": summary.elapsed_ms,
    })))
}

/// The two routes, mounted by `server.rs` under `/api/v1`.
pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/admin/fixity/report", get(get_report))
        .route("/admin/fixity/run", post(post_run))
}
