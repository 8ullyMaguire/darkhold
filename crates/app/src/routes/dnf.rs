use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use darkhold_domain::{dnf::DnfReason, AppError};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

use crate::auth::{MaybeSession, RequirePseud};
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;
use darkhold_db::dnf;
use darkhold_domain::ids::{AccountId, PseudId, WorkId};

#[derive(Debug, Deserialize)]
struct DnfRequest {
    reason: String,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    is_public: Option<bool>,
}

#[derive(Debug, Serialize)]
struct DnfResponse {
    id: String,
    account_id: String,
    pseud_id: String,
    work_id: String,
    reason: String,
    note: Option<String>,
    is_public: bool,
    created_at: String,
    updated_at: String,
}

impl From<dnf::DnfRow> for DnfResponse {
    fn from(row: dnf::DnfRow) -> Self {
        Self {
            id: row.id,
            account_id: row.account_id,
            pseud_id: row.pseud_id,
            work_id: row.work_id,
            reason: row.reason,
            note: row.note,
            is_public: row.is_public,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/works/{id}/dnf",
            post(upsert_dnf)
                .put(upsert_dnf)
                .delete(delete_dnf)
                .get(get_dnf),
        )
        .route("/works/{id}/dnf/reasons", get(get_dnf_reasons))
        .route("/works/{id}/dnf/feedback", get(get_dnf_feedback))
        .route("/me/dnf", get(list_my_dnf))
}

#[derive(Debug, Serialize)]
struct DnfReasonsResponse {
    work_id: String,
    reasons: Vec<ReasonCount>,
}

#[derive(Debug, Serialize)]
struct ReasonCount {
    reason: String,
    count: i64,
}

/// Create or update a DNF record for a work.
async fn upsert_dnf(
    pseud: RequirePseud,
    Path(work_id): Path<Uuid>,
    State(state): State<AppState>,
    Json(req): Json<DnfRequest>,
) -> ApiResult<Json<DnfResponse>> {
    let db = state.db();

    let reason_str = DnfReason::from_str(&req.reason)
        .map_err(|e| {
            ApiError(AppError::Validation {
                message: e,
                field_errors: Default::default(),
            })
        })?
        .as_str()
        .to_string();

    let is_public = req.is_public.unwrap_or(false);

    // `works.allow_dnf_feedback` (default FALSE, migration 0073) as the author's opt-in to a *public* statement about their
    // work. Two asymmetries worth naming, because both were decisions rather than defaults:
    //
    // * The gate is on publication, never on recording. A reader may always record that they stopped reading — that row is
    //   their own reading history, and refusing it would make their library lie about what they read on any work whose
    //   author never touched a setting. What the author controls is whether it becomes a statement *about their work*.
    // * It is checked here, in the producer, and not in `list_dnf_for_work`: a row already published while the door was open
    //   stays published. Turning the setting off stops new statements; it does not erase evidence of old ones, and a
    //   retroactive hide would rewrite what other readers were told.
    if is_public {
        let allowed = dnf::work_allows_dnf_feedback(state.db(), WorkId::from_uuid(work_id))
            .await
            .map_err(|e| ApiError(AppError::Internal(e)))?;
        if !allowed {
            return Err(ApiError(AppError::field(
                "is_public",
                "this work does not accept public did-not-finish feedback; record it privately instead",
            )));
        }
    }

    let now = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();

    dnf::upsert_dnf(
        db,
        AccountId::from_uuid(pseud.user.account_id.as_uuid()),
        PseudId::from_uuid(pseud.pseud_id.as_uuid()),
        WorkId::from_uuid(work_id),
        &reason_str,
        req.note.as_deref(),
        is_public,
        &now,
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e)))?;

    let row = dnf::read_dnf(
        db,
        PseudId::from_uuid(pseud.pseud_id.as_uuid()),
        WorkId::from_uuid(work_id),
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e)))?
    .ok_or_else(|| ApiError(AppError::NotFound { resource: "dnf" }))?;

    Ok(Json(row.into()))
}

/// Remove the caller's DNF record for a work.
async fn delete_dnf(
    pseud: RequirePseud,
    Path(work_id): Path<Uuid>,
    State(state): State<AppState>,
) -> ApiResult<StatusCode> {
    let now = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();

    let deleted = dnf::delete_dnf(
        state.db(),
        PseudId::from_uuid(pseud.pseud_id.as_uuid()),
        WorkId::from_uuid(work_id),
        &now,
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e)))?;

    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError(AppError::NotFound { resource: "dnf" }))
    }
}

/// Get the caller's own DNF record for a work.
async fn get_dnf(
    pseud: RequirePseud,
    Path(work_id): Path<Uuid>,
    State(state): State<AppState>,
) -> ApiResult<Json<DnfResponse>> {
    let row = dnf::read_dnf(
        state.db(),
        PseudId::from_uuid(pseud.pseud_id.as_uuid()),
        WorkId::from_uuid(work_id),
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e)))?
    .ok_or_else(|| ApiError(AppError::NotFound { resource: "dnf" }))?;

    Ok(Json(row.into()))
}

/// Get aggregated public DNF reason counts for a work.
async fn get_dnf_reasons(
    _session: MaybeSession,
    Path(work_id): Path<Uuid>,
    State(state): State<AppState>,
) -> ApiResult<Json<DnfReasonsResponse>> {
    let counts = dnf::aggregate_dnf_counts(state.db(), WorkId::from_uuid(work_id))
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?;

    let reasons = counts
        .into_iter()
        .map(|(reason, count)| ReasonCount { reason, count })
        .collect();

    Ok(Json(DnfReasonsResponse {
        work_id: work_id.to_string(),
        reasons,
    }))
}

/// The author's view of a work's did-not-finish feedback.
///
/// This is the sentence behind the numbers `GET /works/{id}/dnf/reasons` already published: an author who sees "41 readers
/// dropped this for slow pacing" can ask why, and read what the readers who chose to publish said.
///
/// **Published rows only, and the author is not an exception.** `is_public` defaults to false and is the reader's own
/// choice; a private note is their reading history, not the work's property. The store therefore has no `include_private`
/// parameter at all: a parameter that must always be false is one somebody eventually passes true.
///
/// A caller who is not a contributor is answered `404`, exactly as they would be for a work they cannot read. That keeps one
/// rule — "you see what reading the work gives you" — and it carries no information about the author's setting, so the
/// refusal cannot be used to probe whether a work accepts feedback.
async fn get_dnf_feedback(
    pseud: RequirePseud,
    Path(work_id): Path<Uuid>,
    State(state): State<AppState>,
) -> ApiResult<Json<DnfFeedbackResponse>> {
    let work = darkhold_db::content::find_work(state.db(), WorkId::from_uuid(work_id))
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?
        .ok_or_else(|| ApiError(AppError::NotFound { resource: "work" }))?;

    if work.owner_pseud_id.to_string() != pseud.pseud_id.to_string() {
        return Err(ApiError(AppError::NotFound { resource: "work" }));
    }

    let rows = dnf::list_dnf_for_work(state.db(), WorkId::from_uuid(work_id))
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?;

    Ok(Json(DnfFeedbackResponse {
        work_id: work_id.to_string(),
        accepts_feedback: dnf::work_allows_dnf_feedback(state.db(), WorkId::from_uuid(work_id))
            .await
            .map_err(|e| ApiError(AppError::Internal(e)))?,
        items: rows.into_iter().map(DnfFeedbackItem::from).collect(),
    }))
}

/// The feedback list and the setting that governs new entries, in one answer — an author asking "what did readers say" needs
/// to know whether more can arrive, and two round trips would let the two answers disagree.
#[derive(Debug, Serialize)]
struct DnfFeedbackResponse {
    work_id: String,
    accepts_feedback: bool,
    items: Vec<DnfFeedbackItem>,
}

/// One published DNF record, as its author sees it.
///
/// The pseudonym is here and the account id is not: the author needs to know which reader to answer, and an account id is a
/// identity the reader never chose to publish. Everything else is what the reader wrote.
#[derive(Debug, Serialize)]
struct DnfFeedbackItem {
    pseud_id: String,
    reason: String,
    note: Option<String>,
    created_at: String,
    updated_at: String,
}

impl From<dnf::DnfRow> for DnfFeedbackItem {
    fn from(row: dnf::DnfRow) -> Self {
        Self {
            pseud_id: row.pseud_id,
            reason: row.reason,
            note: row.note,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

/// List all DNF records for the authenticated pseud.
async fn list_my_dnf(
    pseud: RequirePseud,
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<DnfResponse>>> {
    let rows = dnf::list_dnf_by_pseud(state.db(), PseudId::from_uuid(pseud.pseud_id.as_uuid()))
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?;

    Ok(Json(rows.into_iter().map(Into::into).collect()))
}
