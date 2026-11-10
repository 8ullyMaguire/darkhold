//! `GET|PUT|DELETE /lens` — §0.3 curator lenses over HTTP (M45-40).
//!
//! Four refusals carry this module, and each one is a different sentence:
//!
//! 1. **Publishing needs the rec-blogger role** — a lens is a view other readers are asked to trust, and the role is what
//!    makes that ask meaningful. A plain reader gets `403`, not `404`: the door is public and they can see it.
//! 2. **A lens is published by its owner** — the handle in the path must be the caller's own, or `403`. Publishing in
//!    somebody else's name is claiming to speak for them.
//! 3. **The operator's lens does not exist** — `403` on the way in and `404` on the way out, because §0.3 forbids the
//!    operator's lens being inferable from any user-facing label, and a public catalogue ordered by it is the strongest
//!    label there is. The refusal is written here rather than in config, so no setting can turn it on by accident.
//! 4. **An unpublished lens is `404`, the same as one that never existed** — a `403` would confirm that a curator under
//!    that handle has a view, which is a fact about them.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::auth::{MaybeSession, RequireSession};
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;
use darkhold_domain::error::AppError;

/// One published lens, as the list serves it.
#[derive(Debug, Serialize)]
pub struct LensView {
    /// The handle the view is published under.
    pub handle: String,
    /// One sentence from the curator about what the view is.
    pub blurb: String,
    /// When it was first published, RFC 3339.
    pub published_at: String,
}

/// The document `GET /lens/{handle}` serves.
#[derive(Debug, Serialize)]
pub struct LensPage {
    /// The lens itself.
    pub lens: LensDetail,
    /// The works, in the lens's order.
    pub works: Vec<darkhold_db::search::SearchResult>,
}

/// The lens, plus what its order is based on.
#[derive(Debug, Serialize)]
pub struct LensDetail {
    /// The handle the view is published under.
    pub handle: String,
    /// One sentence from the curator about what the view is.
    pub blurb: String,
    /// When it was first published, RFC 3339.
    pub published_at: String,
    /// `taste` when the order is the curator's kudos overlap, `catalogue` when they have no signal in this set.
    ///
    /// A lens with no signal is still useful — it is the catalogue, and saying so is the difference between an order the
    /// reader can trust and one they cannot see the reason for.
    pub basis: darkhold_db::lens::LensBasis,
}

/// The body of a publish request.
#[derive(Debug, Deserialize)]
pub struct LensRequest {
    /// One sentence about the view. Required: a list ordered by something the reader cannot see is the thing §0.3 forbids.
    #[serde(default)]
    pub blurb: String,
}

/// The query a lens page accepts.
#[derive(Debug, Deserialize)]
pub struct LensQuery {
    /// How many works to serve. Clamped rather than refused: a page size is not a state change.
    #[serde(default)]
    pub limit: Option<i64>,
}

/// This module's routes.
pub fn router() -> Router<AppState> {
    Router::new().route("/lens", get(list_lenses)).route(
        "/lens/{handle}",
        get(read_lens).put(publish_lens).delete(withdraw_lens),
    )
}

/// The operator's account id, if one is configured.
fn operator_account(state: &AppState) -> Option<String> {
    state
        .config()
        .administration
        .operator_account_id
        .as_ref()
        .map(|id| id.to_string())
}

/// `GET /lens` — every published lens. Public: the list is what a reader consults before trusting one.
async fn list_lenses(
    State(state): State<AppState>,
    // The list is the same for everybody today, and it still takes the session: every public door in this router knows who
    // is asking, so the reader who is entitled to a different list can get one without a second door.
    MaybeSession(_session): MaybeSession,
) -> ApiResult<Json<serde_json::Value>> {
    let operator = operator_account(&state);
    let lenses = darkhold_db::lens::published(state.db())
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?;
    Ok(Json(serde_json::json!({
        // §0.3 in the list as well: a lens that cannot be read must not be named, or the list becomes the label the
        // refusal exists to prevent.
        "lenses": lenses
            .into_iter()
            .filter(|lens| operator.as_deref() != Some(lens.account_id.as_str()))
            .map(|lens| LensView {
                handle: lens.handle,
                blurb: lens.blurb,
                published_at: lens.published_at,
            })
            .collect::<Vec<_>>(),
    })))
}

/// `GET /lens/{handle}` — one lens's page.
async fn read_lens(
    State(state): State<AppState>,
    MaybeSession(session): MaybeSession,
    Path(handle): Path<String>,
    Query(query): Query<LensQuery>,
) -> ApiResult<Json<LensPage>> {
    let lens = darkhold_db::lens::find(state.db(), &handle)
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?;
    let Some(lens) = lens else {
        // Same answer for "never published" and "no such curator": the difference is a fact about a person.
        return Err(ApiError(AppError::NotFound { resource: "lens" }));
    };
    if operator_account(&state).as_deref() == Some(lens.account_id.as_str()) {
        // §0.3. Recorded here, at the read door as well as the write door, because a lens that cannot be published but
        // could still be read would be a published lens with extra steps.
        return Err(ApiError(AppError::NotFound { resource: "lens" }));
    }
    let viewer = session.as_ref().map(|s| s.account_id.to_string());
    let (works, basis) = darkhold_db::lens::ranked_for(
        state.db(),
        &lens.account_id,
        viewer.as_deref(),
        query.limit.unwrap_or(50),
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e)))?;
    Ok(Json(LensPage {
        lens: LensDetail {
            handle: lens.handle,
            blurb: lens.blurb,
            published_at: lens.published_at,
            basis,
        },
        works,
    }))
}

/// `PUT /lens/{handle}` — publish or edit the caller's own lens.
async fn publish_lens(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(handle): Path<String>,
    Json(request): Json<LensRequest>,
) -> ApiResult<StatusCode> {
    let account = user.account_id.to_string();
    let mine = darkhold_db::lens::handles_for(state.db(), &account)
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?;
    if !mine.iter().any(|own| own.eq_ignore_ascii_case(&handle)) {
        // 403, not 404: the lens the caller asked to publish is theirs to see and theirs to decline.
        return Err(ApiError(AppError::AccessDenied));
    }
    if operator_account(&state).as_deref() == Some(account.as_str()) {
        // §0.3, at the write door: the operator's lens is not publishable at all, however it would be spelled.
        return Err(ApiError(AppError::AccessDenied));
    }
    let curator = darkhold_db::media_resilience::is_active_curator(state.db(), &account)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    if !curator {
        return Err(ApiError(AppError::AccessDenied));
    }
    let blurb = request.blurb.trim();
    if blurb.is_empty() || blurb.chars().count() > 280 {
        return Err(ApiError(AppError::field(
            "blurb",
            "A lens needs one sentence (1..=280 characters) about what the view is.",
        )));
    }
    darkhold_db::lens::publish(state.db(), &account, blurb)
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?;
    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE /lens/{handle}` — withdraw the caller's own lens.
async fn withdraw_lens(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(handle): Path<String>,
) -> ApiResult<StatusCode> {
    let account = user.account_id.to_string();
    let mine = darkhold_db::lens::handles_for(state.db(), &account)
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?;
    if !mine.iter().any(|own| own.eq_ignore_ascii_case(&handle)) {
        return Err(ApiError(AppError::AccessDenied));
    }
    if !darkhold_db::lens::withdraw(state.db(), &account)
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?
    {
        return Err(ApiError(AppError::NotFound { resource: "lens" }));
    }
    Ok(StatusCode::NO_CONTENT)
}
