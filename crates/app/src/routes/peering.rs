//! M69-02 — who this instance could peer with, and who it can never peer with.
//!
//! Two doors, both operator-only:
//!
//! * `GET /api/v1/operator/peering/suggestions` — this instance's own fingerprint compared against every peer fingerprint
//!   an operator has recorded, with the veto applied before any score.
//! * `PUT /api/v1/operator/peering/fingerprints` — record what a peer published. Nothing here fetches: an instance that
//!   peeked at strangers on its own would be making a contact the operator did not ask for, and the operator is the one who
//!   decides who is a stranger.
//!
//! Both are closed unless `[discovery] peering_suggestions` is true. The list says which instances this one would pair
//! with, which is a statement about the instance; a default-on statement is one nobody chose.
//!
//! # Why the veto is not a column on the list
//!
//! A vetoed peer is not "0.0 similar". It is ineligible, and a table that sorts by a score would put it next to peers whose
//! score *means* something. The response carries `verdict` per peer and the ranking puts suggestions first, then vetoes,
//! then near misses (see `peering::suggest`) — the order is the answer, and the reason sentence names the rule.

use axum::extract::State;
use axum::routing::{get, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use darkhold_domain::peering;

use crate::auth::RequireSession;
use crate::http::{ApiError, ApiResult};
use crate::routes::discovery::require_operator;
use crate::state::AppState;
use darkhold_domain::error::AppError;

/// The peering routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/operator/peering/suggestions", get(suggestions))
        .route("/operator/peering/fingerprints", put(record_fingerprint))
}

/// The closed door: 404 rather than 403, so a reader cannot learn the surface exists.
fn closed(state: &AppState) -> Option<ApiError> {
    (!state.config().discovery.peering_suggestions).then(|| {
        ApiError(AppError::NotFound {
            resource: "peering suggestions",
        })
    })
}

async fn suggestions(
    State(state): State<AppState>,
    session: RequireSession,
) -> ApiResult<Json<Value>> {
    if let Some(error) = closed(&state) {
        return Err(error);
    }
    require_operator(&state, &session.0)?;

    let own = crate::routes::well_known::current(&state)
        .await?
        .ok_or(ApiError(AppError::NotFound {
            resource: "public taste fingerprint",
        }))?;
    let peers = darkhold_db::peer_fingerprints::list(state.db())
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;

    let documents: Vec<_> = peers.iter().map(|(fingerprint, _)| fingerprint.clone()).collect();
    let verdicts = peering::suggest(&own, &documents, peering::MAX_SUGGESTIONS);
    let fetched: std::collections::BTreeMap<&str, &str> = peers
        .iter()
        .map(|(fingerprint, at)| (fingerprint.instance_id.as_str(), at.as_str()))
        .collect();

    Ok(Json(json!({
        "instance": own.instance_id,
        "enforced": own.enforced,
        "floor": peering::MIN_SIMILARITY,
        "candidates": verdicts.iter().map(|verdict| json!({
            "instance_id": verdict.instance_id(),
            "verdict": match verdict {
                peering::Verdict::Suggested { .. } => "suggested",
                peering::Verdict::NotEnough { .. } => "not_enough",
                peering::Verdict::Collision { .. } => "blocked",
                peering::Verdict::Unmeasured { .. } => "unmeasured",
            },
            "similarity": verdict.similarity(),
            "reason": verdict.reason(),
            "fetched_at": fetched.get(verdict.instance_id()),
        })).collect::<Vec<Value>>(),
        "note": "a collision is a veto, not a low score: no similarity makes an instance with a different hardcoded layer a peer",
    })))
}

#[derive(Debug, Deserialize)]
struct RecordRequest {
    /// The peer's whole `/.well-known/taste-fingerprint.json` document, as a string.
    document: String,
}

async fn record_fingerprint(
    State(state): State<AppState>,
    session: RequireSession,
    Json(request): Json<RecordRequest>,
) -> ApiResult<Json<Value>> {
    if let Some(error) = closed(&state) {
        return Err(error);
    }
    require_operator(&state, &session.0)?;

    let fetched_at = darkhold_db::identity::now_rfc3339();
    let fingerprint = darkhold_db::peer_fingerprints::record(state.db(), &request.document, &fetched_at)
        .await
        .map_err(|error| {
            ApiError(AppError::Validation {
                message: error.to_string(),
                field_errors: std::collections::BTreeMap::new(),
            })
        })?;

    Ok(Json(json!({
        "instance_id": fingerprint.instance_id,
        "generated_at": fingerprint.generated_at,
        "tags": fingerprint.tags.len(),
        "enforced": fingerprint.enforced,
        "fetched_at": fetched_at,
    })))
}
