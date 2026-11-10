//! Quality signals — the writer §32.3's filter dimension never had (M70-13).
//!
//! The schema, the vocabulary, the bounds and the search filters all exist: `min_quality:750` and
//! `quality:editorial_review:750` compile to SQL over `quality_signals`, the "Hidden gems" surface declares the table as a data
//! need, and §32.1 describes a signal as "typed, sourced, recomputable, 0..=1000". The only INSERT of a signal in the tree was in
//! `milestone_22`'s own fixture, and the deployed table held **0 rows** — so every quality filter answered "nothing" for the whole
//! catalogue, which to the person typing it reads as "no such work".
//!
//! Two doors, and the split is the decision:
//!
//! * `POST /admin/works/{id}/quality-signals` — an **operator records a judgement**. A signal is somebody's claim about a work, so
//!   it carries the account that made it as its `source`; that is what makes it re-assertable and what stops two operators silently
//!   overwriting each other. A re-record by the same operator **replaces** their row: the composite is a weighted mean, so an extra
//!   row for the same judgement doubles its weight and outvotes every other signal.
//! * `GET /works/{id}/quality` — the same rows a reader can already filter by, plus the composite the filter computes. Public,
//!   because the filter exposes the aggregate anyway; hiding the components while filtering on them is the worse half of that
//!   choice. The composite uses the filter's own formula, so `0.8` here and `min_quality:0.8` in search describe one instance.
//!
//! `editorial_review`, `quorum_distinction` and `maturity` are judgements, which is why they come through this door rather than from
//! a recompute: a computed `editorial_review` would be an editorial review nobody performed. `completeness` and `reader_positivity`
//! are the two a pass could derive; they are not derived here, and the roadmap says so, because their mappings are policy rather
//! than arithmetic and inventing them in a route is how a scoring philosophy gets decided by accident.

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use darkhold_domain::media::QualitySignalKind;

use crate::auth::{MaybeSession, RequireSession};
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/works/{id}/quality", get(read))
        .route("/admin/works/{id}/quality-signals", post(record))
}

/// Does this work exist (and is it not deleted)? The read is public, so an unknown id must be `404` rather than an empty list that
/// looks like "this work has no quality signals".
async fn work_exists(state: &AppState, id: &str) -> ApiResult<bool> {
    let Ok(work_id) = id.parse::<darkhold_domain::WorkId>() else {
        return Ok(false);
    };
    let found = darkhold_db::content::find_work(state.db(), work_id)
        .await
        .map_err(ApiError::from)?;
    Ok(found.is_some())
}

/// `GET /works/{id}/quality` — the components and the composite.
async fn read(
    State(state): State<AppState>,
    // Public, and the extractor says so: the inventory check requires a public route to declare that a session *may* be present,
    // because a route that silently ignores a caller is how an audience rule goes wrong without being written down.
    MaybeSession(_session): MaybeSession,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    if !work_exists(&state, &id).await? {
        return Err(ApiError(darkhold_domain::AppError::NotFound {
            resource: "work",
        }));
    }
    let signals = darkhold_db::media::work_quality_signals(state.db(), &id)
        .await
        .map_err(|error| ApiError(darkhold_domain::AppError::Internal(error)))?;
    let composite = darkhold_db::media::quality_composite(state.db(), &id)
        .await
        .map_err(|error| ApiError(darkhold_domain::AppError::Internal(error)))?;
    Ok(Json(json!({
        "work_id": id,
        "composite": composite,
        "signals": signals
            .iter()
            .map(|signal| json!({
                "kind": signal.signal_kind,
                "value": signal.value,
                "weight": signal.weight,
                "source": signal.source,
                "computed_at": signal.computed_at,
            }))
            .collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
struct RecordBody {
    kind: String,
    value: i64,
    #[serde(default)]
    weight: Option<i64>,
}

/// `POST /admin/works/{id}/quality-signals` — an operator records (or corrects) one judgement.
async fn record(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
    Json(body): Json<RecordBody>,
) -> ApiResult<(axum::http::StatusCode, Json<serde_json::Value>)> {
    // ADR 0003: one decision, one place. A caller with no standing gets `404` here, not from a second check of my own.
    crate::routes::admin_scope::require_admin_of(&state, &user, None).await?;

    // The vocabulary is the domain's; a route that accepted a string would be a second list of kinds to keep in step.
    if body.kind.parse::<QualitySignalKind>().is_err() {
        return Err(ApiError(darkhold_domain::AppError::field(
            "kind",
            "unknown kind",
        )));
    }
    if !(0..=1000).contains(&body.value) {
        return Err(ApiError(darkhold_domain::AppError::field(
            "value",
            "must be between 0 and 1000",
        )));
    }
    let weight = body.weight.unwrap_or(1);
    if !(1..=1000).contains(&weight) {
        // A zero weight is a signal that exists and is counted by nothing: it would satisfy `quality:kind:…` filters (which look
        // for existence) while contributing to no composite. Refuse it rather than record a row with two meanings.
        return Err(ApiError(darkhold_domain::AppError::field(
            "weight",
            "must be between 1 and 1000",
        )));
    }
    if !work_exists(&state, &id).await? {
        return Err(ApiError(darkhold_domain::AppError::NotFound {
            resource: "work",
        }));
    }

    let source = format!("operator:{}", user.account_id);
    let signal =
        darkhold_db::media::store_signal(state.db(), &id, &body.kind, body.value, weight, &source)
            .await
            .map_err(|error| ApiError(darkhold_domain::AppError::Internal(error)))?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(json!({
            "signal": {
                "kind": signal.signal_kind,
                "value": signal.value,
                "weight": signal.weight,
                "source": signal.source,
                "computed_at": signal.computed_at,
            }
        })),
    ))
}
