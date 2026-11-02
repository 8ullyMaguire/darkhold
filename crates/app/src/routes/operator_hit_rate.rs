//! §53.5 — the hit rate gets a door, so the named measure is readable (`M66-09`).
//!
//! The metric has existed as `darkhold_db::hit_rate` since M53 and **nothing served it**: the
//! operator's one number could only be obtained by writing a query by hand against the instance's
//! database. A measure nobody can read is not a measure.
//!
//! Four decisions, each of which the repository's own conventions forced:
//!
//! 1. **A non-operator gets `404`, not `403`.** `decision_service::require_operator` argues it and
//!    `admin_discovery` repeats it: a 403 confirms the endpoint exists, and for an operator-only
//!    measure the existence is itself a disclosure — a reader probing the door learns that the
//!    instance keeps a per-reader hit rate at all.
//! 2. **A disabled extension is also `404`, deliberately indistinguishable from a refusal.** The
//!    surface is a registered extension (§59.3), so an instance that does not run it answers with
//!    `MODULE_UNAVAILABLE` from `module_mount` before this handler is reached — and that response is
//!    a 404 for the same reason. Two different codes ("you may not" and "this instance does not")
//!    would let a caller map an operator's configuration from outside.
//! 3. **`rate` is `null` and `defined` is `false` when nothing was shown**, never `0.0`. §53.5: a
//!    zero reads as total failure and invites a change to a recipe that has not been tested yet.
//! 4. **The window is bounded.** `window_days` is clamped to 1..=365 rather than taken from the
//!    caller: an unbounded window would let a caller ask for "all of history" and turn a rate into a
//!    lifetime average that no recipe change can move.
//!
//! The measure is about **the caller's own pseud** — the operator's feed, not every reader's. §53.5
//! defines it over the operator's impressions precisely so it is not a surveillance surface.

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::RequireSession;
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

use darkhold_domain::error::AppError;

/// Bounds of the window the door will answer for.
const MIN_WINDOW_DAYS: i64 = 1;
const MAX_WINDOW_DAYS: i64 = 365;
const DEFAULT_WINDOW_DAYS: i64 = 30;

pub fn router() -> Router<AppState> {
    Router::new().route("/operator/hit-rate", get(get_hit_rate))
}

#[derive(Debug, Deserialize)]
pub struct HitRateQuery {
    /// Days back from now. Clamped; not rejected, because a client asking for 400 days wants "a year".
    pub window_days: Option<i64>,
}

/// `GET /api/v1/operator/hit-rate` — the operator's own §53.5 number over a window.
pub async fn get_hit_rate(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Query(query): Query<HitRateQuery>,
) -> ApiResult<Json<Value>> {
    if state.config().administration.operator_account_id != Some(user.account_id) {
        tracing::debug!(
            operator_configured = state.config().administration.operator_account_id.is_some(),
            "the hit-rate door was reached by an account that is not the operator"
        );
        return Err(ApiError(AppError::NotFound {
            resource: "hit rate",
        }));
    }

    // The pseud whose impressions are measured: the one the session is acting as, else the
    // account's first. `sessions.active_pseud_id` is a session property, so an account with
    // several pseuds must not have an arbitrary one picked.
    let pseud = match user.pseud_id.as_ref() {
        Some(p) => p.as_uuid(),
        None => match darkhold_db::identity::pseuds_for_account(state.db(), user.account_id)
            .await
            .map_err(|e| ApiError(AppError::Internal(e)))?
            .first()
        {
            Some(first) => first.id.as_uuid(),
            None => {
                // No pseud means no feed, which means no impressions. That is a configuration
                // state rather than a request error, and the door says so rather than inventing
                // an account-wide number that §53.5 does not define.
                tracing::warn!(
                    account = %user.account_id,
                    "the operator account holds no pseud, so the hit rate has nothing to measure"
                );
                return Err(ApiError(AppError::NotFound {
                    resource: "hit rate",
                }));
            }
        },
    };

    let days = query
        .window_days
        .unwrap_or(DEFAULT_WINDOW_DAYS)
        .clamp(MIN_WINDOW_DAYS, MAX_WINDOW_DAYS);
    let until = time::OffsetDateTime::now_utc().unix_timestamp();
    let since = until - days * 86_400;

    let rate = darkhold_db::hit_rate::hit_rate(state.db(), pseud, since, until)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;

    // `defined` is the field a page needs: a reader of this JSON must not have to infer "no
    // impressions" from a null, because null and zero are the distinction §53.5 exists to keep.
    Ok(Json(json!({
        "shown": rate.shown,
        "hits": rate.hits,
        "rate": rate.rate,
        "defined": rate.rate.is_some(),
        "window_days": days,
        "since": since,
        "until": until,
    })))
}
