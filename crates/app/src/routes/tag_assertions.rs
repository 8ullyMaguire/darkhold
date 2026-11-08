//! Tagging a work: assert, confirm, retract, and the assertions a work carries.
//!
//! These are the writers `crates/db/src/tag_assertions.rs` was built for and that nothing called. A reader could see tags and
//! could never add one, which made "a model may only propose" a rule about a proposal path that did not exist.
//!
//! # The rules the doors carry, and where each one comes from
//!
//! * **Proposals are not facts.** `GET` returns every assertion to an operator and only the confirmed ones to everybody else,
//!   because a model's guess rendered as a tag is the thing `is_proposal()` exists to prevent.
//! * **A model cannot confirm.** There is no request body that makes this route write `inferred` + `confirmed`: the claimant is
//!   always `Human` (the tag is added by the person who asked), and `ConfirmLevel` has no `Model` arm, so the sentence "a model
//!   confirmed a tag" is one this module cannot express rather than one it remembers to refuse.
//! * **`author` versus `curator` is derived, not claimed.** Whether a caller's tag is the author's own or a fan's is decided by
//!   whether their pseud owns the work, never by a field they send.
//! * **Retraction belongs to the work's owner and to the operator.** A stranger withdrawing a tag is the same act as adding one
//!   they were never asked for, and the refusal comes from the operator gate, which answers `404` like every other operator
//!   surface (ADR 0030).

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::RequireSession;
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;
use darkhold_db::tag_assertions::{self, Claimant, ConfirmLevel, TagAssertion};
use darkhold_domain::error::AppError;

/// Tagging a work.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/works/{id}/tag-assertions", get(list))
        .route("/works/{id}/tags/{node}/assert", post(assert))
        .route("/works/{id}/tags/{node}/confirm", post(confirm))
        .route("/works/{id}/tags/{node}", delete(retract))
}

fn internal(error: anyhow::Error) -> ApiError {
    ApiError(AppError::Internal(error))
}

fn invalid(message: impl Into<String>) -> ApiError {
    ApiError(AppError::Validation {
        message: message.into(),
        field_errors: std::collections::BTreeMap::new(),
    })
}

fn view(row: &TagAssertion) -> Value {
    json!({
        "node_id": row.node_id,
        "source": row.source,
        "confirmation": row.confirmation,
        "confidence": row.confidence,
        "added_at": row.added_at,
        // Named rather than left to the client: a proposal is a different kind of sentence from a tag, and a page that has to
        // infer that from `source == "inferred" && confirmation == "unconfirmed"` will get it wrong once and show a guess.
        "proposal": row.is_proposal(),
    })
}

/// Every assertion on a work — proposals included for an operator, confirmed rows for everyone else.
async fn list(
    State(state): State<AppState>,
    session: RequireSession,
    Path(work): Path<String>,
) -> ApiResult<Json<Value>> {
    let rows = tag_assertions::assertions_for_work(state.db(), &work)
        .await
        .map_err(internal)?;
    let is_operator = crate::routes::discovery::require_operator(&state, &session.0).is_ok();
    let kept: Vec<&TagAssertion> = rows
        .iter()
        .filter(|row| is_operator || !row.is_proposal())
        .collect();
    Ok(Json(json!({
        "work_id": work,
        "assertions": kept.iter().map(|row| view(row)).collect::<Vec<Value>>(),
        // Said out loud: the answer is not the whole table, and a reader who knows a model has been guessing should be able
        // to tell that something is being withheld rather than conclude the work has no tags.
        "proposals_withheld": rows.len() - kept.len(),
    })))
}

#[derive(Debug, Default, Deserialize)]
struct AssertBody {
    /// 0..=1. Absent means 1.0 — a person typing a tag is not uncertain about it.
    #[serde(default)]
    confidence: Option<f64>,
}

async fn assert_tag(
    state: &AppState,
    user: &crate::auth::SessionUser,
    work: &str,
    node: &str,
    confidence: f64,
) -> ApiResult<Json<Value>> {
    let account = user.account_id.to_string();
    let owned = darkhold_db::orphan::owned_by(state.db(), work, &account)
        .await
        .map_err(internal)?;
    // The claimant is always `Human`: these doors are the app's, and an import or a model writes through its own path. The
    // author/curator distinction is derived from ownership rather than taken from the request.
    tag_assertions::assert_tag(state.db(), work, node, Claimant::Human, owned, confidence)
        .await
        .map_err(internal)?;
    Ok(Json(json!({
        "work_id": work,
        "node_id": node,
        "source": Claimant::Human.source(owned),
        "confirmation": Claimant::Human.confirmation(),
        "confidence": confidence.clamp(0.0, 1.0),
        "note": if owned {
            "added as the author of this work"
        } else {
            "added as a reader; a curator or other readers confirm it"
        },
    })))
}

async fn assert(
    State(state): State<AppState>,
    session: RequireSession,
    Path((work, node)): Path<(String, String)>,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    let request: AssertBody = if body.is_empty() {
        AssertBody::default()
    } else {
        serde_json::from_slice(&body).map_err(|error| invalid(format!("body: {error}")))?
    };
    let confidence = request.confidence.unwrap_or(1.0);
    if !(0.0..=1.0).contains(&confidence) || confidence.is_nan() {
        return Err(invalid(format!(
            "confidence: {confidence} is outside 0..=1; it says how strongly this row asserts the tag"
        )));
    }
    assert_tag(&state, &session.0, &work, &node, confidence).await
}

#[derive(Debug, Default, Deserialize)]
struct ConfirmBody {
    /// `reader` (default) or `wrangler`. A wrangler confirmation is the curator's verdict and needs the operator.
    #[serde(default)]
    level: Option<String>,
}

async fn confirm(
    State(state): State<AppState>,
    session: RequireSession,
    Path((work, node)): Path<(String, String)>,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    let request: ConfirmBody = if body.is_empty() {
        ConfirmBody::default()
    } else {
        serde_json::from_slice(&body).map_err(|error| invalid(format!("body: {error}")))?
    };
    let level = match request.level.as_deref().unwrap_or("reader") {
        "reader" => ConfirmLevel::Reader,
        "wrangler" => {
            // The instance's operator shape is `404` (ADR 0030), and it is right here too: a caller may see a tag without
            // being entitled to know whether this instance has an operator who speaks for the taxonomy.
            crate::routes::discovery::require_operator(&state, &session.0)?;
            ConfirmLevel::Wrangler
        }
        other => {
            return Err(invalid(format!(
            "level: '{other}' is not a confirmation level; the levels are 'reader' and 'wrangler'"
        )))
        }
    };
    let changed = tag_assertions::confirm(state.db(), &work, &node, level)
        .await
        .map_err(internal)?;
    if !changed {
        // The same answer for "no such work" and "no such tag on it": the first names a work the caller may not know exists.
        return Err(ApiError(AppError::NotFound { resource: "tag" }));
    }
    Ok(Json(json!({
        "work_id": work,
        "node_id": node,
        "confirmation": match level {
            ConfirmLevel::Reader => "reader",
            ConfirmLevel::Wrangler => "wrangler",
        },
    })))
}

async fn retract(
    State(state): State<AppState>,
    session: RequireSession,
    Path((work, node)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let account = session.0.account_id.to_string();
    let owned = darkhold_db::orphan::owned_by(state.db(), &work, &account)
        .await
        .map_err(internal)?;
    if !owned {
        crate::routes::discovery::require_operator(&state, &session.0)?;
    }
    let removed = tag_assertions::retract(state.db(), &work, &node)
        .await
        .map_err(internal)?;
    if !removed {
        return Err(ApiError(AppError::NotFound { resource: "tag" }));
    }
    Ok(Json(
        json!({ "work_id": work, "node_id": node, "retracted": true }),
    ))
}
