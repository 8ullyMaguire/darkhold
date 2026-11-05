//! M70-01 — orphaning over HTTP.
//!
//! ```text
//! POST /works/:id/orphan   detach this work from its author, keep it online
//! ```
//!
//! # Why this is not a `DELETE`
//!
//! The obvious shape is `DELETE /works/:id`, and it would be wrong twice: it says the work is being removed when
//! the work is the thing being kept, and it invites a client to retry it — orphaning has one transition, and a
//! retry has to be told "already done" rather than being allowed to append a second suffix.
//!
//! # The refusal is a 404, not a 403
//!
//! A stranger asking to orphan someone else's work gets the answer a missing work gets. A `403` would confirm that
//! the id names a real work, which is exactly what a stranger is trying to learn.

use axum::extract::{Path, State};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{json, Value};

use darkhold_domain::error::AppError;

use crate::auth::RequireSession;
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

/// The orphaning route.
pub fn router() -> Router<AppState> {
    Router::new().route("/works/{id}/orphan", post(orphan))
}

fn invalid(message: impl Into<String>) -> ApiError {
    ApiError(AppError::Validation {
        message: message.into(),
        field_errors: std::collections::BTreeMap::new(),
    })
}

async fn orphan(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(work): Path<String>,
) -> ApiResult<Json<Value>> {
    let account = user.account_id.to_string();
    if !darkhold_db::orphan::owned_by(state.db(), &work, &account).await? {
        // Same answer for "not yours" and "not there".
        return Err(invalid("no work with that id"));
    }
    match darkhold_db::orphan::orphan_work(state.db(), &work).await? {
        Some(label) => Ok(Json(json!({
            "orphaned": true,
            "author_label": label,
        }))),
        None => Err(invalid("this work is already orphaned")),
    }
}
