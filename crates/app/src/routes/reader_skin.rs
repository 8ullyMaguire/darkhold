//! M70-06 — reader skins over HTTP.
//!
//! ```text
//! GET    /api/v1/me/skin        metadata: whether there is one, how big, when it changed
//! PUT    /api/v1/me/skin        {"css": "..."} — validated, then stored
//! DELETE /api/v1/me/skin        back to the site's own interface
//! GET    /api/v1/me/skin.css    the stylesheet itself, text/css, for this session only
//! ```
//!
//! Three decisions:
//!
//! * **The refusal is the validator's own sentence.** A generic `VALIDATION_FAILED` would leave the reader guessing
//!   which of six rules they broke; `Refusal::message()` names the construct to remove.
//! * **`.css` is served with `text/css` and `Cache-Control: private, no-cache`.** Private because it belongs to one
//!   reader; no-cache because a reader who edits their skin expects the next page load to show the edit, and a
//!   stylesheet that lags behind its editor reads as a broken save.
//! * **No account appears in any path.** There is nothing to pass, so there is nothing to guess.

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use darkhold_domain::error::AppError;
use darkhold_domain::reader_skin::{check, Refusal};

use crate::auth::RequireSession;
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

/// Where a reader's stylesheet lives.
pub const CSS_PATH: &str = "/me/skin.css";

/// The skin routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/me/skin", get(describe).put(write).delete(clear))
        .route(CSS_PATH, get(stylesheet))
}

/// The request body: just the CSS.
#[derive(Debug, Deserialize)]
struct SkinBody {
    css: String,
}

fn refused(refusal: &Refusal) -> ApiError {
    let mut field_errors = std::collections::BTreeMap::new();
    field_errors.insert("css".to_owned(), refusal.message());
    ApiError(AppError::Validation {
        message: refusal.message(),
        field_errors,
    })
}

async fn describe(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<Value>> {
    let account = user.account_id.to_string();
    let stored = darkhold_db::reader_skin::get(state.db(), &account).await?;
    Ok(Json(match stored {
        Some(skin) => json!({
            "present": true,
            "bytes": skin.css.len(),
            "updated_at": skin.updated_at,
            "url": format!("/api/v1{CSS_PATH}"),
        }),
        None => json!({ "present": false, "url": format!("/api/v1{CSS_PATH}") }),
    }))
}

async fn write(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Json(body): Json<SkinBody>,
) -> ApiResult<Json<Value>> {
    let skin = check(&body.css).map_err(|refusal| refused(&refusal))?;
    let account = user.account_id.to_string();
    darkhold_db::reader_skin::put(state.db(), &account, skin.css()).await?;
    Ok(Json(json!({ "present": true, "bytes": skin.css().len() })))
}

async fn clear(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<Value>> {
    let account = user.account_id.to_string();
    let removed = darkhold_db::reader_skin::remove(state.db(), &account).await?;
    Ok(Json(json!({ "present": false, "removed": removed })))
}

async fn stylesheet(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Response> {
    let account = user.account_id.to_string();
    let stored = darkhold_db::reader_skin::get(state.db(), &account).await?;
    let Some(skin) = stored else {
        // 404 rather than an empty stylesheet: the shell links to this only when `describe` says there is one, and
        // an empty `text/css` response would look like a skin that does nothing.
        return Err(ApiError(AppError::NotFound { resource: "skin" }));
    };
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8".to_owned()),
            (header::CACHE_CONTROL, "private, no-cache".to_owned()),
            // The document is the reader's own bytes; nothing about the request should let a browser sniff it into
            // anything else.
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_owned()),
        ],
        skin.css,
    )
        .into_response())
}
