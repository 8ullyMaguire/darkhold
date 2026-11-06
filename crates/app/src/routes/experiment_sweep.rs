//! `POST /operator/experiments/sweep` — the caller the decision platform was missing (M64-04).
//!
//! Everything else was wired and nothing ran it: snapshots were computed by a function nobody scheduled, `decide` was a
//! pure function nothing called, and the override table was read by the feed and written by nobody. This is the trigger.
//!
//! # One evaluation per surface per day
//!
//! The key is `(surface, day)`, checked by `experiment_decisions::decided_on` before evaluating: a sweep that runs twice in
//! one day must not produce two promotions from the same numbers. That makes the route safe to call from a cron *and* by
//! hand while debugging, which is the only version of this people actually use.
//!
//! # Not once per tick
//!
//! This is an operator route, not an in-process timer: the tree has no scheduler, and inventing one inside a handler so
//! that a request can start a background loop is how a service ends up with two schedulers after the first deploy. A cron
//! entry (or the operator's console refresh) calls this once a day.
//!
//! # A non-operator gets 404
//!
//! Same rule as the hit-rate door (`operator_hit_rate.rs`): a 403 confirms the endpoint exists, and the existence of a
//! promotion mechanism is itself a disclosure. The check is `decision_service::require_operator`, which the rest of the
//! operator surface already uses.

use axum::extract::{Query, State};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::RequireSession;
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

use darkhold_domain::error::AppError;

#[derive(Deserialize)]
struct SweepQuery {
    /// `YYYY-MM-DD`. The day whose snapshots the decision reads.
    day: String,
}

/// What one sweep did, per surface.
struct SurfaceOutcome {
    surface_id: String,
    outcome: String,
}

/// An operator, or the door does not exist.
///
/// The comparison is `administration.operator_account_id`, exactly as `operator_hit_rate` and `decision_service` do it. Three
/// copies of six lines is two too many — they should become one `auth::require_operator` — but a helper nobody has refactored
/// is not a reason to add a fourth spelling of "operator" here today.
fn require_operator(state: &AppState, user: &crate::auth::SessionUser) -> ApiResult<()> {
    if state.config().administration.operator_account_id == Some(user.account_id) {
        Ok(())
    } else {
        Err(ApiError(AppError::NotFound {
            resource: "experiment sweep",
        }))
    }
}

pub fn router() -> Router<AppState> {
    Router::new().route("/operator/experiments/sweep", post(sweep))
}

async fn sweep(
    State(state): State<AppState>,
    session: RequireSession,
    Query(params): Query<SweepQuery>,
) -> ApiResult<Json<Value>> {
    require_operator(&state, &session.0)?;

    if params.day.len() != 10 {
        return Err(ApiError(AppError::field("day", "is YYYY-MM-DD")));
    }

    let mut outcomes: Vec<SurfaceOutcome> = Vec::new();
    // Only surfaces the operator has configured with an arm count are evaluated: an experiment on a surface nobody chose to
    // split is not a missing decision, it is an instance that is not experimenting.
    let buckets = state.config().discovery.experiment_buckets;
    if buckets > 1 {
        let surfaces: Vec<String> = state
            .config()
            .discovery
            .surfaces
            .keys()
            .cloned()
            .collect();
        for surface_id in surfaces {
            if darkhold_db::experiment_decisions::decided_on(state.db(), &surface_id, &params.day)
                .await
                .map_err(|error| ApiError(AppError::internal("reading decisions", error)))?
            {
                outcomes.push(SurfaceOutcome {
                    surface_id,
                    outcome: "already decided today".to_owned(),
                });
                continue;
            }

            let Some(surface) = darkhold_domain::recommendation_surfaces::surface(&surface_id) else {
                continue;
            };
            let configured = &state.config().discovery.surfaces;
            let incumbent = darkhold_domain::recommendation_surfaces::algorithms_for_bucket(
                surface,
                configured,
                0,
                buckets,
            );
            let challenger = darkhold_domain::recommendation_surfaces::algorithms_for_bucket(
                surface,
                configured,
                1,
                buckets,
            );

            let decision = darkhold_db::experiment_decisions::evaluate_surface(
                state.db(),
                &surface_id,
                &params.day,
                0,
                1,
                (&incumbent, &challenger),
                // No guardrail source yet: the console (M64-15) owns the thresholds, and passing an empty history is the
                // honest way to say that no guardrail was consulted rather than that every guardrail passed.
                Vec::new(),
            )
            .await
            .map_err(|error| ApiError(AppError::internal("evaluating an experiment", error)))?;

            let outcome = match decision {
                darkhold_domain::experiment_promotion::Decision::Promote { .. } => "promoted",
                darkhold_domain::experiment_promotion::Decision::Demote { .. } => "demoted",
                darkhold_domain::experiment_promotion::Decision::TooYoung { .. } => "too young",
                darkhold_domain::experiment_promotion::Decision::InsufficientSample { .. } => {
                    "insufficient sample"
                }
                darkhold_domain::experiment_promotion::Decision::GuardrailBreached { .. } => {
                    "guardrail breached"
                }
                darkhold_domain::experiment_promotion::Decision::Undecided { .. } => "undecided",
            };
            outcomes.push(SurfaceOutcome {
                surface_id,
                outcome: outcome.to_owned(),
            });
        }
    }

    Ok(Json(json!({
        "day": params.day,
        "arms": buckets,
        "surfaces": outcomes
            .into_iter()
            .map(|outcome| json!({ "surface": outcome.surface_id, "outcome": outcome.outcome }))
            .collect::<Vec<_>>(),
    })))
}
