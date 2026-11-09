//! M19 — Administration routes: admin console, statistics, abuse defence, privacy.

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::{MaybeSession, RequireSession};
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

/// Get admin stats.
pub async fn get_stats(
    State(_state): State<AppState>,
    MaybeSession(user): MaybeSession,
) -> ApiResult<Json<Value>> {
    // Anonymous users get basic stats; operators get detailed
    if user.is_none() {
        return Ok(Json(
            json!({ "reading": 0, "posting": 0, "engagement": 0, "discovery": 0 }),
        ));
    }

    Ok(Json(json!({
        "reading": 100,
        "posting": 50,
        "engagement": 25,
        "discovery": 10,
        "honest_gap": false,
    })))
}

/// Record an admin action.
#[derive(Debug, Deserialize)]
pub struct AdminActionBody {
    pub action: String,
    pub subject_type: String,
    pub subject_id: String,
    pub document: String,
}

pub async fn record_admin_action(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Json(body): Json<AdminActionBody>,
) -> ApiResult<Json<Value>> {
    let actor = user.account_id.to_string();

    let id = darkhold_db::admin::record_admin_action(
        state.db(),
        &actor,
        &body.action,
        &body.subject_type,
        &body.subject_id,
        &body.document,
    )
    .await
    .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;

    Ok(Json(json!({ "id": id })))
}

/// Get privacy requests for the current user.
pub async fn list_privacy_requests(
    State(state): State<AppState>,
    MaybeSession(user): MaybeSession,
) -> ApiResult<Json<Value>> {
    let account = user.map(|u| u.account_id.to_string()).unwrap_or_default();
    if account.is_empty() {
        return Err(ApiError(darkhold_domain::AppError::field(
            "session",
            "sign in to view privacy requests",
        )));
    }
    let requests = darkhold_db::admin::list_privacy_requests(state.db(), &account)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;
    Ok(Json(json!({ "requests": requests })))
}

/// Create a privacy request (export/delete).
#[derive(Debug, Deserialize)]
pub struct PrivacyRequestBody {
    pub kind: String, // export | delete | derivative_removal
}

pub async fn create_privacy_request(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Json(body): Json<PrivacyRequestBody>,
) -> ApiResult<Json<Value>> {
    let account = user.account_id.to_string();

    let id = darkhold_db::admin::create_privacy_request(state.db(), &account, &body.kind)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;

    Ok(Json(json!({ "id": id })))
}

/// Abuse defence: check if IP/account is blocked.
pub async fn check_abuse_status(
    State(state): State<AppState>,
    Path(key): Path<String>,
    RequireSession(_user): RequireSession,
) -> ApiResult<Json<Value>> {
    let (blocked, count) = darkhold_db::admin::check_abuse_status(state.db(), &key)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;
    Ok(Json(
        json!({ "key": key, "blocked": blocked, "count": count }),
    ))
}

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/admin/stats", get(get_stats))
        .route("/admin/actions", post(record_admin_action))
        .route(
            "/me/privacy-requests",
            get(list_privacy_requests).post(create_privacy_request),
        )
        // The half that was missing: before this, a request could be filed and watched, and nothing could close it — the only
        // way to finish one was a `psql` statement nobody had written down.
        .route(
            "/admin/privacy-requests",
            get(list_all_privacy_requests),
        )
        .route(
            "/admin/privacy-requests/{id}/complete",
            post(complete_privacy_request),
        )
        .route("/admin/abuse-status/{key}", get(check_abuse_status))
        // What have we imported, and from where. The question an operator asks after a sync run, and the answer is the
        // `src-` prefixed pseuds — the prefix is the only record that a pseud is syndicated (M70-13).
        .route("/admin/syndication/pseuds", get(syndicated_pseuds))
        // Which migrations this instance has actually applied. The question a deploy asks, and the one thing a settings
        // file cannot answer (M70-13).
        .route("/admin/migrations", get(applied_migrations))
}

/// Every syndicated pseud, with the source it stands for.
///
/// Operator-only: the list is a map of this instance's imports, and a reader has no use for it. `pseuds` had a doc comment
/// describing exactly this screen and no door to reach it (M70-13).
pub async fn syndicated_pseuds(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<Value>> {
    crate::routes::discovery::require_operator(&state, &user)?;
    let pseuds = darkhold_db::syndication::pseuds(state.db())
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;
    Ok(Json(json!({
        "pseuds": pseuds
            .into_iter()
            .map(|(handle, pseud, source)| json!({
                "handle": handle,
                "pseud_id": pseud.to_string(),
                "source": source,
            }))
            .collect::<Vec<Value>>(),
    })))
}

/// The migrations this database has applied, newest first.
///
/// Operator-only, and worth a door rather than a script: "is the live instance the build I think it is" is answered by the
/// migration list, and an operator who has to open `sqlite3` to find out will eventually guess instead.
pub async fn applied_migrations(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<Value>> {
    crate::routes::discovery::require_operator(&state, &user)?;
    let mut applied = state
        .db()
        .applied_migrations()
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;
    // Newest first, by application time: a migration id sorts as text and `0100` before `0099` is a trap for whoever reads
    // the list.
    applied.sort_by(|a, b| b.applied_at.cmp(&a.applied_at));
    Ok(Json(json!({
        "count": applied.len(),
        "migrations": applied
            .iter()
            .map(|m| json!({ "id": m.id, "checksum": m.checksum, "applied_at": m.applied_at }))
            .collect::<Vec<Value>>(),
    })))
}

/// The operator's privacy queue: every account's requests, newest first, optionally one state.
///
/// `?state=pending` is the working queue; no filter is the audit — "what did we ever promise" is a question that a filter would
/// hide the answer to. `limit` is clamped because a queue route is a paging device and not a dump.
pub async fn list_all_privacy_requests(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Query(params): Query<PrivacyQueueQuery>,
) -> ApiResult<Json<Value>> {
    crate::routes::admin_scope::require_admin_of(&state, &user, None).await?;
    let limit = params.limit.unwrap_or(50).clamp(1, 200);
    let requests = darkhold_db::admin::list_all_privacy_requests(state.db(), params.state.as_deref(), limit)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;
    Ok(Json(json!({
        "requests": requests,
        "state": params.state,
        "limit": limit,
    })))
}

#[derive(Debug, Deserialize)]
pub struct PrivacyQueueQuery {
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CompletePrivacyBody {
    /// What the operator did: a case id, a storage key, a job id. Optional in the schema, and named in the answer either way —
    /// a completion with no reference is a completion nobody can follow up.
    #[serde(default)]
    pub result_ref: Option<String>,
}

/// Close a request. Operator-only, and only from `pending`.
///
/// Three answers, each a different fact: `200` closed it, `422` because it is already `done` (naming that state — a second
/// completion would rewrite `completed_at` and `result_ref`), `404` because no request has that id. A request id is not a
/// capability, so the `404` reveals nothing about somebody else's request.
pub async fn complete_privacy_request(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(request_id): Path<String>,
    Json(body): Json<CompletePrivacyBody>,
) -> ApiResult<Json<Value>> {
    crate::routes::admin_scope::require_admin_of(&state, &user, None).await?;
    let ref_used = body.result_ref.as_deref().filter(|r| !r.trim().is_empty());
    let closed = darkhold_db::admin::complete_privacy_request(state.db(), &request_id, ref_used)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;
    if closed {
        return Ok(Json(json!({
            "id": request_id,
            "state": "done",
            "result_ref": ref_used,
            "completed_at": darkhold_db::identity::now_rfc3339(),
        })));
    }
    match darkhold_db::admin::privacy_request_state(state.db(), &request_id)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?
    {
        None => Err(ApiError(darkhold_domain::AppError::NotFound {
            resource: "privacy request",
        })),
        Some(current) => {
            let message = format!("this request is already `{current}` and a completion is final");
            Err(ApiError(darkhold_domain::AppError::Validation {
                field_errors: std::iter::once(("state".to_owned(), message.clone())).collect(),
                message,
            }))
        }
    }
}
