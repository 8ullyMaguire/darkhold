//! M69-07 — the evacuation doors.
//!
//! * `POST /api/v1/operator/evacuation` — announce a shutdown; the plan is computed, not typed.
//! * `GET  /api/v1/operator/evacuation` — the notice, the placements, and the receipt so far.
//! * `POST /api/v1/operator/evacuation/assignments/{id}/settle` — a recipient took it, or did not.
//! * `POST /api/v1/operator/evacuation/close` — withdrawn or completed.
//!
//! # Why the recipients are computed
//!
//! The same rule that decides who may mirror a work (M69-05) decides who may inherit one: `mirroring::eligible_peers` against
//! the published fingerprints, with the same floor. An evacuation where a distant peer takes an author's work is not an
//! evacuation, it is a redistribution — and the operator choosing the recipients by hand is how a shutdown becomes a copy of
//! somebody's catalogue onto a host nobody vetted.
//!
//! # Why the plan is recorded rather than recomputed
//!
//! The plan is a snapshot of what the instance believed at the moment it announced: these works, these peers, these
//! similarities. Recomputing it after the fact would rewrite history every time a peer's fingerprint moved, and a receipt that
//! disagrees with the plan it came from is a receipt an author cannot use to find their work.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::RequireSession;
use crate::http::{ApiError, ApiResult};
use crate::routes::discovery::require_operator;
use crate::routes::well_known;
use crate::state::AppState;
use darkhold_domain::error::AppError;
use darkhold_domain::evacuation::{self, WorkNeed};
use darkhold_domain::mirroring::{eligible_peers, MIN_MIRROR_SIMILARITY};

/// The evacuation routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/operator/evacuation", get(read).post(announce))
        .route("/operator/evacuation/close", post(close))
        .route("/operator/evacuation/assignments/{id}/settle", post(settle))
}

fn closed(state: &AppState) -> Option<ApiError> {
    (!state.config().discovery.evacuation).then(|| {
        ApiError(AppError::NotFound {
            resource: "evacuation",
        })
    })
}

fn internal(error: anyhow::Error) -> ApiError {
    ApiError(AppError::Internal(error))
}

fn parse<T: serde::de::DeserializeOwned>(body: &Bytes) -> Result<T, ApiError> {
    serde_json::from_slice(body).map_err(|error| {
        let mut field_errors = std::collections::BTreeMap::new();
        field_errors.insert("body".to_owned(), error.to_string());
        ApiError(AppError::Validation {
            message: "the request body is not valid".to_owned(),
            field_errors,
        })
    })
}

fn validation(message: String) -> ApiError {
    ApiError(AppError::Validation {
        message,
        field_errors: std::collections::BTreeMap::new(),
    })
}

/// The works this instance would hand over: published, public, undeleted.
async fn exportable_works(state: &AppState, limit: i64) -> anyhow::Result<Vec<WorkNeed>> {
    let sql = state.db().sql(
        "SELECT id, title FROM works
          WHERE deleted_at IS NULL AND lifecycle = 'published' AND visibility = 'public'
          ORDER BY created_at, id LIMIT ?",
        "SELECT id, title FROM works
          WHERE deleted_at IS NULL AND lifecycle = 'published' AND visibility = 'public'
          ORDER BY created_at, id LIMIT $1",
    );
    let rows: Vec<(String, String)> = match state.db().backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(limit)
                .fetch_all(state.db().sqlite_pool().expect("sqlite"))
                .await?
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(limit)
                .fetch_all(state.db().postgres_pool().expect("postgres"))
                .await?
        }
    };
    Ok(rows
        .into_iter()
        .map(|(work_id, title)| WorkNeed { work_id, title })
        .collect())
}

/// The peers eligible to receive, by the same rule that governs mirrors.
async fn recipients(
    state: &AppState,
) -> ApiResult<Vec<darkhold_domain::mirroring::Eligible>> {
    let Some(own) = well_known::current(state).await? else {
        return Err(validation(
            "this instance has published no taste fingerprint, so no peer can be checked against it".to_owned(),
        ));
    };
    let peers = darkhold_db::peer_fingerprints::list(state.db())
        .await
        .map_err(internal)?;
    let documents: Vec<_> = peers.into_iter().map(|(fingerprint, _)| fingerprint).collect();
    let floor = state
        .config()
        .discovery
        .mirror_floor
        .unwrap_or(MIN_MIRROR_SIMILARITY)
        .max(MIN_MIRROR_SIMILARITY);
    Ok(eligible_peers(&own, &documents, floor))
}

#[derive(Debug, Deserialize)]
struct AnnounceRequest {
    reason: String,
    #[serde(default)]
    note: String,
    /// How many works to plan for. Defaults to 100, capped at 1000 — an evacuation larger than that is a batch job, and
    /// pretending otherwise is how one request times out half-way through a shutdown.
    #[serde(default)]
    limit: Option<i64>,
}

async fn announce(
    State(state): State<AppState>,
    session: RequireSession,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    if let Some(error) = closed(&state) {
        return Err(error);
    }
    require_operator(&state, &session.0)?;
    let request: AnnounceRequest = parse(&body)?;
    let limit = request.limit.unwrap_or(100).clamp(1, 1000);

    let peers = recipients(&state).await?;
    if peers.is_empty() {
        // Said plainly rather than announcing an evacuation with nowhere to go: a receipt of zero is worse than a refusal,
        // because it looks like the work was placed.
        return Err(validation(
            "no peer is eligible to receive this instance's work; publish a fingerprint or record peer fingerprints first"
                .to_owned(),
        ));
    }
    let works = exportable_works(&state, limit).await.map_err(internal)?;
    let plan = evacuation::plan(&works, &peers);
    let now = darkhold_db::identity::now_rfc3339();
    let announced_by = session.0.account_id.to_string();

    let (notice, assignments) = darkhold_db::evacuation::announce(
        state.db(),
        &request.reason,
        &announced_by,
        &request.note,
        &plan,
        &now,
    )
    .await
    .map_err(|error| validation(error.to_string()))?;

    Ok(Json(json!({
        "notice": {
            "id": notice.id,
            "reason": notice.reason,
            "state": notice.state,
            "announced_at": notice.announced_at,
        },
        "plan": {
            "placed": plan.placements.len(),
            "unplaced": plan.unplaced.len(),
            "load": plan.load().iter().map(|(instance, count)| json!({
                "recipient": instance,
                "works": count,
            })).collect::<Vec<Value>>(),
        },
        "assignments": assignments.iter().map(|assignment| json!({
            "id": assignment.id,
            "work_id": assignment.work_id,
            "title": assignment.title,
            "recipient": assignment.recipient_instance,
            "similarity_bp": assignment.similarity_bp,
            "state": assignment.state,
        })).collect::<Vec<Value>>(),
        "unplaced": plan.unplaced.iter().map(|row| json!({
            "work_id": row.work_id,
            "title": row.title,
            "reason": row.reason,
        })).collect::<Vec<Value>>(),
    })))
}

async fn read(State(state): State<AppState>, session: RequireSession) -> ApiResult<Json<Value>> {
    if let Some(error) = closed(&state) {
        return Err(error);
    }
    require_operator(&state, &session.0)?;
    let Some(notice) = darkhold_db::evacuation::open_notice(state.db())
        .await
        .map_err(internal)?
    else {
        return Ok(Json(json!({ "notice": Value::Null, "note": "no evacuation is announced" })));
    };
    let assignments = darkhold_db::evacuation::assignments_for(state.db(), &notice.id)
        .await
        .map_err(internal)?;
    let receipt = darkhold_db::evacuation::receipt(state.db(), &notice.id)
        .await
        .map_err(internal)?;
    let forks = receipt.forks();

    Ok(Json(json!({
        "notice": {
            "id": notice.id,
            "reason": notice.reason,
            "note": notice.note,
            "announced_by": notice.announced_by,
            "announced_at": notice.announced_at,
            "state": notice.state,
        },
        "assignments": assignments.iter().map(|assignment| json!({
            "id": assignment.id,
            "work_id": assignment.work_id,
            "title": assignment.title,
            "recipient": assignment.recipient_instance,
            "similarity_bp": assignment.similarity_bp,
            "state": assignment.state,
            "settled_at": assignment.settled_at,
            "settle_note": assignment.settle_note,
        })).collect::<Vec<Value>>(),
        "offered": assignments.iter().filter(|row| row.state == "offered").count(),
        "receipt": {
            "taken": receipt.load().iter().map(|(instance, count)| json!({
                "recipient": instance,
                "works": count,
            })).collect::<Vec<Value>>(),
            "accepted": receipt.accepted.len(),
            "declined": receipt.declined.len(),
            "forks": forks.iter().map(|(work_id, instances)| json!({
                "work_id": work_id,
                "claimed_by": instances,
            })).collect::<Vec<Value>>(),
        },
    })))
}

#[derive(Debug, Deserialize)]
struct SettleRequest {
    state: String,
    #[serde(default)]
    note: String,
}

async fn settle(
    State(state): State<AppState>,
    session: RequireSession,
    Path(id): Path<String>,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    if let Some(error) = closed(&state) {
        return Err(error);
    }
    require_operator(&state, &session.0)?;
    let request: SettleRequest = parse(&body)?;
    let now = darkhold_db::identity::now_rfc3339();
    let assignment = darkhold_db::evacuation::settle(
        state.db(),
        &id,
        request.state.trim(),
        request.note.trim(),
        &now,
    )
    .await
    // A second acceptance answers 422, not 409: this instance's error vocabulary has no generic conflict variant (its 409 is
    // `RevisionConflict`, which names two version numbers this refusal does not have). The message names the holder, which is
    // what an operator needs; inventing a version pair to reach a 409 would be a lie in the payload.
    .map_err(|error| validation(error.to_string()))?;

    Ok(Json(json!({
        "assignment": {
            "id": assignment.id,
            "work_id": assignment.work_id,
            "title": assignment.title,
            "recipient": assignment.recipient_instance,
            "state": assignment.state,
            "settled_at": assignment.settled_at,
        },
        "note": if assignment.state == "accepted" {
            "recorded as the one home this work has"
        } else {
            "declined: the work needs another recipient before the instance closes"
        },
    })))
}

#[derive(Debug, Deserialize)]
struct CloseRequest {
    state: String,
}

async fn close(
    State(state): State<AppState>,
    session: RequireSession,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    if let Some(error) = closed(&state) {
        return Err(error);
    }
    require_operator(&state, &session.0)?;
    let request: CloseRequest = parse(&body)?;
    let now = darkhold_db::identity::now_rfc3339();
    let notice = darkhold_db::evacuation::close(state.db(), request.state.trim(), &now)
        .await
        .map_err(|error| validation(error.to_string()))?;
    Ok(Json(json!({
        "notice": {
            "id": notice.id,
            "state": notice.state,
            "completed_at": notice.completed_at,
        },
        "note": "the notice and its placements stay: an evacuation that was called off is part of the instance's history",
    })))
}
