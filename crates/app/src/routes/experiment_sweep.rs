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
use axum::routing::{get, post};
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
    Router::new()
        .route("/operator/experiments/sweep", post(sweep))
        .route("/operator/experiments", get(overview))
        .route("/operator/experiments/{surface}/hold", post(hold_surface).delete(release_surface))
        .route("/operator/experiments/{surface}/promote", post(promote_by_hand))
        .route("/operator/experiments/{surface}/demote", post(demote_by_hand))
}

#[derive(Deserialize)]
struct HoldRequest {
    reason: String,
}

#[derive(Deserialize)]
struct PromoteRequest {
    /// The algorithms the operator is installing. Named explicitly rather than taken from an arm: a hand promotion is a
    /// decision about *which set*, and inferring it from an arm index would make the operator's intent a guess.
    algorithms: Vec<String>,
}

async fn hold_surface(
    State(state): State<AppState>,
    session: RequireSession,
    axum::extract::Path(surface): axum::extract::Path<String>,
    Json(body): Json<HoldRequest>,
) -> ApiResult<Json<Value>> {
    require_operator(&state, &session.0)?;
    if body.reason.trim().is_empty() {
        return Err(ApiError(AppError::field(
            "reason",
            "is required, and is why the hold exists",
        )));
    }
    darkhold_db::experiment_decisions::hold(state.db(), &surface, body.reason.trim())
        .await
        .map_err(|error| ApiError(AppError::internal("holding a surface", error)))?;
    Ok(Json(json!({ "surface": surface, "held": true })))
}

async fn release_surface(
    State(state): State<AppState>,
    session: RequireSession,
    axum::extract::Path(surface): axum::extract::Path<String>,
) -> ApiResult<Json<Value>> {
    require_operator(&state, &session.0)?;
    let released = darkhold_db::experiment_decisions::release(state.db(), &surface)
        .await
        .map_err(|error| ApiError(AppError::internal("releasing a hold", error)))?;
    Ok(Json(json!({ "surface": surface, "released": released })))
}

async fn promote_by_hand(
    State(state): State<AppState>,
    session: RequireSession,
    axum::extract::Path(surface): axum::extract::Path<String>,
    Json(body): Json<PromoteRequest>,
) -> ApiResult<Json<Value>> {
    require_operator(&state, &session.0)?;
    decide_by_hand(&state, &surface, &body.algorithms, "promote").await
}

async fn demote_by_hand(
    State(state): State<AppState>,
    session: RequireSession,
    axum::extract::Path(surface): axum::extract::Path<String>,
) -> ApiResult<Json<Value>> {
    require_operator(&state, &session.0)?;
    decide_by_hand(&state, &surface, &[], "demote").await
}

/// A hand decision goes through the same door as an automatic one: a row in `experiment_decisions` with `source = 'manual'`,
/// and the override installed or cleared exactly as an automatic verdict would.
async fn decide_by_hand(
    state: &AppState,
    surface_id: &str,
    algorithms: &[String],
    decision: &str,
) -> ApiResult<Json<Value>> {
    // A promoted set that no engine implements would install an override the feed cannot honour and the surface would serve
    // nothing. Refused here, by name, rather than at the first request — and the same check `[discovery.surfaces]` gets at
    // load, because a hand action is a second way to write the same setting.
    if decision == "promote" {
        if algorithms.is_empty() {
            return Err(ApiError(AppError::field(
                "algorithms",
                "cannot be empty on a promotion",
            )));
        }
        for algorithm in algorithms {
            if darkhold_domain::recommendation_surfaces::engine_reason(algorithm).is_none() {
                return Err(ApiError(AppError::field(
                    "algorithms",
                    format!("no engine implements `{algorithm}`"),
                )));
            }
        }
    }

    let before = state
        .config()
        .discovery
        .surfaces
        .get(surface_id)
        .cloned()
        .unwrap_or_default();

    let id = darkhold_db::experiment_decisions::record_manual(
        state.db(),
        surface_id,
        &today(),
        decision,
        &before,
        algorithms,
    )
    .await
    .map_err(|error| ApiError(AppError::internal("recording a hand decision", error)))?;

    Ok(Json(json!({
        "decision": decision,
        "id": id,
        "algorithms": algorithms,
    })))
}

/// The day the hand action is recorded against. UTC, because every snapshot day is UTC and a local date would file a decision
/// under a day whose numbers it was not read from.
fn today() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    let days = now / 86_400;
    // Civil-from-days (Howard Hinnant's algorithm), which avoids a chrono dependency for one line.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

#[derive(Deserialize)]
struct OverviewQuery {
    /// `YYYY-MM-DD`. Absent means the most recent day that has a snapshot.
    day: Option<String>,
}

/// Every live experiment, with its numbers — M64-15's read surface.
///
/// The console is not a new system: the numbers are the snapshot rows, the swap is the override row, and the argument is the
/// decision row written when it happened. This composes the three rather than restating them, so the console cannot show a
/// surface running one set of algorithms while the feed runs another.
async fn overview(
    State(state): State<AppState>,
    session: RequireSession,
    Query(params): Query<OverviewQuery>,
) -> ApiResult<Json<Value>> {
    require_operator(&state, &session.0)?;
    if let Some(day) = params.day.as_deref() {
        if day.len() != 10 {
            return Err(ApiError(AppError::field("day", "is YYYY-MM-DD")));
        }
    }

    let snapshots = match params.day.as_deref() {
        Some(day) => darkhold_db::experiment_snapshots::day_snapshots(state.db(), day)
            .await
            .map_err(|error| ApiError(AppError::internal("reading snapshots", error)))?,
        None => darkhold_db::experiment_snapshots::latest_snapshots(state.db())
            .await
            .map_err(|error| ApiError(AppError::internal("reading snapshots", error)))?
            .unwrap_or_default(),
    };

    // One entry per surface that has any arm with numbers, plus the arms' aggregates. `BTreeMap` keeps the response ordered
    // by surface id, so a console diff between two days is readable.
    let mut by_surface: std::collections::BTreeMap<String, Vec<Value>> =
        std::collections::BTreeMap::new();
    let mut arms_by_surface: std::collections::BTreeMap<String, i64> =
        std::collections::BTreeMap::new();
    for snapshot in &snapshots {
        let rate = if snapshot.impressions > 0 {
            snapshot.events as f64 / snapshot.impressions as f64
        } else {
            0.0
        };
        by_surface
            .entry(snapshot.surface_id.clone())
            .or_default()
            .push(json!({
                "arm": snapshot.arm,
                "impressions": snapshot.impressions,
                "readers": snapshot.readers,
                "guest_readers": snapshot.guest_readers,
                "events": snapshot.events,
                "conversion_rate": rate,
            }));
        arms_by_surface
            .entry(snapshot.surface_id.clone())
            .and_modify(|count| *count = (*count).max(snapshot.arm + 1))
            .or_insert(snapshot.arm + 1);
    }

    let mut surfaces = Vec::new();
    for (surface_id, arms) in by_surface {
        let override_algorithms =
            darkhold_db::experiment_decisions::override_for(state.db(), &surface_id)
                .await
                .map_err(|error| ApiError(AppError::internal("reading an override", error)))?;
        let last_decision = darkhold_db::experiment_decisions::decisions_for(state.db(), &surface_id)
            .await
            .map_err(|error| ApiError(AppError::internal("reading decisions", error)))?
            .into_iter()
            .next();

        surfaces.push(json!({
            "surface": surface_id,
            "arms": arms_by_surface.get(&surface_id).copied().unwrap_or(0),
            "numbers": arms,
            "override": override_algorithms,
            // The argument, verbatim from the row that made it — a console that recomputed the reason could disagree with the
            // record it is displaying.
            "last_decision": last_decision.map(|decision| json!({
                "decision": decision.decision,
                "day": decision.day,
                "lift": decision.lift,
                "evidence": decision.evidence,
                "algorithms_before": decision.algorithms_before,
                "algorithms_after": decision.algorithms_after,
            })),
        }));
    }

    Ok(Json(json!({
        "day": params.day,
        "configured_arms": state.config().discovery.experiment_buckets,
        "surfaces": surfaces,
    })))
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
            // A held surface is skipped and said so: an operator who paused an experiment must not see the sweep quietly
            // deciding it a minute later.
            if let Some(reason) = darkhold_db::experiment_decisions::hold_reason(state.db(), &surface_id)
                .await
                .map_err(|error| ApiError(AppError::internal("reading a hold", error)))?
            {
                outcomes.push(SurfaceOutcome {
                    surface_id,
                    outcome: format!("held: {reason}"),
                });
                continue;
            }
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
                // The guardrails the platform can measure from its own snapshots: a conversion-rate collapse and a traffic
                // collapse, both relative to the incumbent. Empty when the incumbent had no traffic on a day — nothing was
                // measured, and a fabricated pass is worse than a missing day.
                darkhold_db::experiment_decisions::guardrail_history(
                    state.db(),
                    &surface_id,
                    0,
                    1,
                )
                .await
                .map_err(|error| ApiError(AppError::internal("reading guardrails", error)))?,
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
