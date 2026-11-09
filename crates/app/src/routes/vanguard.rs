use crate::auth::{MaybeSession, RequirePseud, RequireSession};
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use darkhold_db::roles;
use darkhold_domain::AppError;
use serde::Deserialize;
use serde_json::{json, Value};

/// Check whether the current user holds the Vanguard role.
pub async fn get_my_vanguard_status(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<Value>> {
    let is_vanguard = roles::is_vanguard(state.db(), &user.account_id.to_string())
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    Ok(Json(json!({ "is_vanguard": is_vanguard })))
}

/// Pin a work to the Vanguard Picks shelf.
#[derive(Debug, Deserialize)]
pub struct PinWorkBody {
    pub pin_reason: String,
    pub message: Option<String>,
}

pub async fn pin_work(
    State(state): State<AppState>,
    RequirePseud { user, .. }: RequirePseud,
    Path(work_id): Path<String>,
    Json(body): Json<PinWorkBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let account_id = user.account_id.to_string();
    let is_vanguard = roles::is_vanguard(state.db(), &account_id)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    if !is_vanguard {
        return Err(ApiError(AppError::AccessDenied));
    }
    let id = roles::pin_work(
        state.db(),
        &account_id,
        &work_id,
        &body.pin_reason,
        body.message.as_deref(),
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

/// Unpin a work.
pub async fn unpin_work(
    State(state): State<AppState>,
    RequirePseud { user, .. }: RequirePseud,
    Path(work_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let account_id = user.account_id.to_string();
    roles::unpin_work(state.db(), &account_id, &work_id)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    Ok(Json(json!({ "unpinned": true })))
}

/// List pins for a work (public).
pub async fn get_pins_for_work(
    State(state): State<AppState>,
    MaybeSession(_session): MaybeSession,
    Path(work_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let pins = roles::list_active_pins(state.db())
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    let filtered: Vec<Value> = pins
        .into_iter()
        .filter(|p| p["work_id"].as_str() == Some(work_id.as_str()))
        .collect();
    Ok(Json(json!({ "pins": filtered })))
}

/// Check whether the current user is an operator (trust level ≥ 5).
async fn require_operator(state: &AppState, user: &crate::auth::SessionUser) -> ApiResult<()> {
    let level = darkhold_db::governance::trust_for(state.db(), &user.account_id.to_string())
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    if level < 5 {
        return Err(ApiError(AppError::AccessDenied));
    }
    Ok(())
}

/// Admin: grant Vanguard role to an account.
#[derive(Debug, Deserialize)]
pub struct GrantVanguardBody {
    pub account_id: String,
    pub selection_method: String,
    pub resonance_score: Option<f64>,
}

pub async fn grant_vanguard(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Json(body): Json<GrantVanguardBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require_operator(&state, &user).await?;
    roles::grant_vanguard(
        state.db(),
        &body.account_id,
        &body.selection_method,
        Some(&user.account_id.to_string()),
        None,
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    Ok((StatusCode::CREATED, Json(json!({ "granted": true }))))
}

/// Admin: revoke Vanguard role from an account.
pub async fn revoke_vanguard(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(account_id): Path<String>,
) -> ApiResult<Json<Value>> {
    require_operator(&state, &user).await?;
    roles::revoke_vanguard(state.db(), &account_id)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    Ok(Json(json!({ "revoked": true })))
}

/// List all current vanguards (admin only).
pub async fn list_vanguards(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<Value>> {
    require_operator(&state, &user).await?;
    let vanguards = roles::list_vanguards(state.db())
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    Ok(Json(json!({ "vanguards": vanguards })))
}

/// Admin: run a selection method and (only when asked) grant the result.
///
/// **The dry run is the default.** A Vanguard can pin works to the front page, so a selection that appointed people as a side
/// effect of asking "who would qualify?" would turn a question into an appointment. `contribution_volume` measures activity;
/// whether activity deserves the pin is the operator's decision, and this door is shaped so the decision is theirs to make with
/// the list in front of them.
///
/// The method is validated by name rather than silently yielding nothing: `resonance_threshold` needs an admin taste profile
/// that nothing on this instance computes, and `admin_appointment` is one account at a time through `POST /vanguards`. An empty
/// list would answer "nobody qualifies" to a question this door never asked.
///
/// Operator check: this file's own `require_operator` (trust ≥ 5), the same predicate as its four sibling doors. There are five
/// spellings of "is this the operator" across the codebase and converging them is its own row — using one here would make this
/// the only door in the module that answers differently from the one beside it.
#[derive(Debug, Deserialize)]
pub struct SelectVanguardsBody {
    pub method: String,
    #[serde(default)]
    pub limit: Option<i64>,
    /// Absent means a dry run. Not `#[serde(default)]`-with-true anywhere: the safe direction for a role grant is the one that
    /// grants nothing.
    #[serde(default)]
    pub apply: Option<bool>,
}

/// The largest list one call may act on.
///
/// A bound rather than a default, and the two are different things: the default is what an operator gets for asking nothing,
/// the bound is what they cannot get past. Both exist.
const MAX_SELECTION: i64 = 100;

pub async fn select_vanguards(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Json(body): Json<SelectVanguardsBody>,
) -> ApiResult<Json<Value>> {
    require_operator(&state, &user).await?;

    if body.method != "contribution_volume" {
        return Err(ApiError(AppError::field(
            "method",
            "this door computes `contribution_volume` only; `admin_appointment` is one account at a time through POST \
             /vanguards, and `resonance_threshold` needs a taste profile this instance does not build",
        )));
    }
    let limit = body.limit.unwrap_or(10).clamp(1, MAX_SELECTION);
    let apply = body.apply.unwrap_or(false);

    let candidates = roles::select_vanguards(state.db(), &body.method, limit)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;

    // Granted only on request, and each grant records the method that picked the account: a Vanguard row that cannot say how it
    // was chosen cannot be re-examined when the method changes.
    let mut granted: Vec<String> = Vec::new();
    if apply {
        let by = user.account_id.to_string();
        for account in &candidates {
            roles::grant_vanguard(
                state.db(),
                account,
                &body.method,
                Some(&by),
                None,
            )
            .await
            .map_err(|e| ApiError(AppError::Internal(e.into())))?;
            granted.push(account.clone());
        }
    }

    Ok(Json(json!({
        "method": body.method,
        "limit": limit,
        "applied": apply,
        "candidates": candidates,
        "granted": granted,
    })))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/vanguard/status", get(get_my_vanguard_status))
        .route(
            "/vanguard/pins/{work_id}",
            get(get_pins_for_work).post(pin_work).delete(unpin_work),
        )
        .route("/vanguards", get(list_vanguards).post(grant_vanguard))
        // A static segment beside `{account_id}`: axum prefers the literal, so this does not shadow `DELETE /vanguards/{id}`.
        .route("/vanguards/select", post(select_vanguards))
        .route("/vanguards/{account_id}", delete(revoke_vanguard))
}
