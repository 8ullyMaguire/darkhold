//! M69-06 — the rescue ring doors.
//!
//! * `POST /api/v1/operator/rescue/copies` — a survivor says what it holds.
//! * `GET  /api/v1/operator/rescue/copies` — what survivors have claimed.
//! * `POST /api/v1/operator/rescue/{dead_peer}` — hunt for one media reference lost at that peer.
//! * `GET  /api/v1/operator/rescue` — every hunt, settled ones included.
//! * `POST /api/v1/operator/rescue/hunts/{id}/resolve` — accept (naming the survivor), reject, abandon.
//!
//! # Recovery is proposed, never performed
//!
//! Nothing here attaches media to a work. A rescue records *who else has it*, and `accepted` records that an operator agrees
//! — the attaching is `media_resilience`'s existing availability-link machinery, where the same act is already audited. A
//! ring that wrote links directly would be a second path into the one table where a wrong image is a visible wrong.
//!
//! # The ambiguity is the feature
//!
//! An `ambiguous` hunt stores no matched instance, and `accept` then *requires* the operator to name one. That is the
//! cheapest possible place to put the decision: two survivors equally close means the hashes cannot tell them apart, and
//! crediting whichever sorted first is how a work ends up pointing at the wrong copy.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::RequireSession;
use crate::http::{ApiError, ApiResult};
use crate::routes::discovery::require_operator;
use crate::state::AppState;
use darkhold_domain::error::AppError;
use darkhold_domain::media_rescue::Wanted;

/// The rescue routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/operator/rescue", get(list).post(hunt))
        .route("/operator/rescue/copies", get(list_copies).post(record_copy))
        .route("/operator/rescue/hunts/{id}/resolve", post(resolve))
}

fn closed(state: &AppState) -> Option<ApiError> {
    (!state.config().discovery.media_rescue).then(|| {
        ApiError(AppError::NotFound {
            resource: "rescue ring",
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

#[derive(Debug, Deserialize)]
struct CopyRequest {
    instance: String,
    content_hash: String,
    #[serde(default)]
    perceptual_hash: Option<String>,
    #[serde(default)]
    width: Option<i64>,
    #[serde(default)]
    height: Option<i64>,
    #[serde(default)]
    media_kind: Option<String>,
}

async fn record_copy(
    State(state): State<AppState>,
    session: RequireSession,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    if let Some(error) = closed(&state) {
        return Err(error);
    }
    require_operator(&state, &session.0)?;
    let request: CopyRequest = parse(&body)?;
    // A content hash is the key, so a non-hash is refused rather than stored: an empty or malformed one would match
    // nothing and would sit in the table looking like a survivor that could have helped.
    if request.content_hash.trim().len() < 16 {
        return Err(validation(
            "a content hash is at least 16 characters; a shorter one cannot be the key of a copy".to_owned(),
        ));
    }
    if let Some(perceptual) = request.perceptual_hash.as_deref() {
        if darkhold_domain::media_resilience::hamming_distance(perceptual, perceptual).is_none() {
            return Err(validation(format!(
                "{perceptual:?} is not a perceptual hash (expected hex, an even number of characters)"
            )));
        }
    }

    let now = darkhold_db::identity::now_rfc3339();
    let copy = darkhold_db::media_rescue::record_copy(
        state.db(),
        &darkhold_db::media_rescue::CopyClaim {
            instance: request.instance.trim(),
            content_hash: request.content_hash.trim(),
            perceptual_hash: request.perceptual_hash.as_deref(),
            width: request.width,
            height: request.height,
            media_kind: request.media_kind.as_deref().unwrap_or("image"),
            now: &now,
        },
    )
    .await
    .map_err(|error| validation(error.to_string()))?;

    Ok(Json(json!({
        "copy": {
            "id": copy.id,
            "instance": copy.instance,
            "content_hash": copy.content_hash,
            "perceptual_hash": copy.perceptual_hash,
            "width": copy.width,
            "height": copy.height,
            "recorded_at": copy.recorded_at,
        },
        "note": "a claim, not a promise: nothing verifies the survivor still serves it, which is why last_seen_at exists",
    })))
}

async fn list_copies(
    State(state): State<AppState>,
    session: RequireSession,
) -> ApiResult<Json<Value>> {
    if let Some(error) = closed(&state) {
        return Err(error);
    }
    require_operator(&state, &session.0)?;
    let rows = darkhold_db::media_rescue::list_copies(state.db(), 200)
        .await
        .map_err(internal)?;
    Ok(Json(json!({
        "copies": rows.iter().map(|copy| json!({
            "instance": copy.instance,
            "content_hash": copy.content_hash,
            "perceptual_hash": copy.perceptual_hash,
            "width": copy.width,
            "height": copy.height,
            "recorded_at": copy.recorded_at,
        })).collect::<Vec<Value>>(),
        "count": rows.len(),
    })))
}

#[derive(Debug, Deserialize)]
struct HuntRequest {
    /// Whose copy is gone. A body field, not a path segment: the peer is a URL, and a URL in a path is two segments — the
    /// first draft of this door took it in the path and every hunt answered 404 because the router never saw one segment.
    dead_peer: String,
    media_reference_id: String,
}

async fn hunt(
    State(state): State<AppState>,
    session: RequireSession,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    if let Some(error) = closed(&state) {
        return Err(error);
    }
    require_operator(&state, &session.0)?;
    let request: HuntRequest = parse(&body)?;

    let reference = darkhold_db::media_resilience::find_media_reference_by_id(
        state.db(),
        &request.media_reference_id,
    )
    .await
    .map_err(internal)?
    .ok_or(ApiError(AppError::NotFound {
        resource: "media reference",
    }))?;

    let wanted = Wanted {
        content_hash: reference.content_hash.clone(),
        perceptual_hash: reference.perceptual_hash.clone(),
        width: reference.width.and_then(|value| u32::try_from(value).ok()),
        height: reference.height.and_then(|value| u32::try_from(value).ok()),
    };
    let threshold = state
        .config()
        .media_resilience
        .perceptual_match_threshold
        .clamp(0, 64) as u32;
    let now = darkhold_db::identity::now_rfc3339();
    if request.dead_peer.trim().is_empty() {
        return Err(validation(
            "a hunt names the peer whose copy is gone; without it the receipt cannot say who lost what".to_owned(),
        ));
    }
    let outcome = darkhold_db::media_rescue::request(
        state.db(),
        request.dead_peer.trim(),
        &request.media_reference_id,
        &wanted,
        threshold,
        &now,
    )
    .await
    .map_err(internal)?;

    Ok(Json(json!({
        "hunt": {
            "id": outcome.id,
            "dead_peer": outcome.dead_peer,
            "media_reference_id": outcome.media_reference_id,
            "outcome": outcome.outcome,
            "matched_instance": outcome.matched_instance,
            "distance": outcome.distance,
            "confidence": outcome.confidence,
            "candidates": serde_json::from_str::<Value>(&outcome.candidates_json).unwrap_or(Value::Null),
            "threshold": outcome.threshold,
            "created_at": outcome.created_at,
        },
        "note": match outcome.outcome.as_str() {
            "exact" => "the identical bytes are at that survivor; accept to record the credit",
            "proposal" => "a perceptually close copy; a proposal, never an attachment -- a curator decides",
            "ambiguous" => "more than one survivor qualifies; the ring will not choose, name the one to credit",
            _ => "nobody on the ring reports these bytes or a close picture",
        },
    })))
}

async fn list(State(state): State<AppState>, session: RequireSession) -> ApiResult<Json<Value>> {
    if let Some(error) = closed(&state) {
        return Err(error);
    }
    require_operator(&state, &session.0)?;
    let rows = darkhold_db::media_rescue::list(state.db(), 200)
        .await
        .map_err(internal)?;
    Ok(Json(json!({
        "hunts": rows.iter().map(|hunt| json!({
            "id": hunt.id,
            "dead_peer": hunt.dead_peer,
            "media_reference_id": hunt.media_reference_id,
            "outcome": hunt.outcome,
            "matched_instance": hunt.matched_instance,
            "distance": hunt.distance,
            "confidence": hunt.confidence,
            "candidates": serde_json::from_str::<Value>(&hunt.candidates_json).unwrap_or(Value::Null),
            "created_at": hunt.created_at,
            "resolved_at": hunt.resolved_at,
            "resolution": hunt.resolution,
        })).collect::<Vec<Value>>(),
        "open": rows.iter().filter(|hunt| hunt.resolved_at.is_none()).count(),
    })))
}

#[derive(Debug, Deserialize)]
struct ResolveRequest {
    resolution: String,
    /// The survivor to credit. Required when accepting a hunt the ring left ambiguous.
    #[serde(default)]
    instance: Option<String>,
}

async fn resolve(
    State(state): State<AppState>,
    session: RequireSession,
    Path(id): Path<String>,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    if let Some(error) = closed(&state) {
        return Err(error);
    }
    require_operator(&state, &session.0)?;
    let request: ResolveRequest = parse(&body)?;

    let hunt = darkhold_db::media_rescue::find(state.db(), &id)
        .await
        .map_err(internal)?
        .ok_or(ApiError(AppError::NotFound {
            resource: "hunt",
        }))?;

    // Accepting names somebody. An exact or proposed hunt already has a name, so the operator may accept without repeating
    // it; an ambiguous one does not, and the ring will not invent one.
    let instance = request
        .instance
        .as_deref()
        .map(str::to_owned)
        .or_else(|| hunt.matched_instance.clone());
    if request.resolution == "accepted" && instance.is_none() {
        return Err(validation(
            "this hunt named no survivor; accepting it needs `instance` naming the one to credit".to_owned(),
        ));
    }

    let now = darkhold_db::identity::now_rfc3339();
    let changed = darkhold_db::media_rescue::resolve(
        state.db(),
        &id,
        request.resolution.trim(),
        &session.0.account_id.to_string(),
        instance.as_deref(),
        &now,
    )
    .await
    .map_err(|error| validation(error.to_string()))?;
    if !changed {
        return Err(ApiError(AppError::NotFound {
            resource: "open hunt",
        }));
    }

    Ok(Json(json!({
        "hunt": id,
        "resolution": request.resolution.trim(),
        "credited": instance,
        "at": now,
        "note": "the ring records who has it; attaching the media is media_resilience's audited path, not this door",
    })))
}
