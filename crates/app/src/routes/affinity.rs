//! M69-09 — the affinity doors.
//!
//! ```text
//! GET  /me/affinity                    my score and the components that produced it
//! GET  /operator/affinity              every scored reader, highest first (+ the distribution)
//! POST /operator/affinity/recompute    one sweep; reports initialised / changed / unchanged / unscored
//! ```
//!
//! # The sweep is operator-triggered, and that is the honest shape
//!
//! This tree has no scheduler: no `tokio::time::interval`, no sweep loop, no queue ticker. A nightly job invented inside a
//! request handler would be a timer per request multiplied by the process count, so the trigger is a door an operator (or
//! their cron) calls, and the interval lives in config as a statement of intent rather than a promise the process keeps.
//! The write is an upsert on the account, so calling it twice repairs instead of doubling, and a second run over
//! unchanged data reports zeros — which is the check that the lookup is not silently matching nothing.
//!
//! # What the reader sees, and what they do not
//!
//! A reader sees their own score and which evidence produced it, including the denominator: 0.62 from one signal and 0.62
//! from three are different claims about them. What a reader does not see is anybody else's score. The ranking is the
//! instance's own view of its readership, and a leaderboard of alignment would turn a reading of evidence into a
//! competition.

use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::http::{ApiError, ApiResult};
use crate::state::AppState;
use darkhold_domain::AppError;

/// The affinity routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/me/affinity", get(my_affinity))
        .route("/operator/affinity", get(operator_view))
        .route("/operator/affinity/recompute", post(recompute))
}

/// My own score, with its components.
async fn my_affinity(
    State(state): State<AppState>,
    crate::auth::RequireSession(user): crate::auth::RequireSession,
) -> ApiResult<Json<Value>> {
    let affinity = darkhold_db::reader_affinity::for_account(state.db(), &user.account_id.to_string())
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?;

    Ok(Json(match affinity {
        Some(row) => json!({
            "scored": true,
            "affinity": row,
            "note": "score_bp is 0..10000; evidence_bp is how much of the three signals it rests on",
        }),
        // No row is not a zero. Saying "0% aligned" to a reader the instance has never measured is a judgement it did
        // not make, and one every new reader would receive on their first day.
        None => json!({
            "scored": false,
            "affinity": null,
            "note": "there is not enough evidence to score you yet: bookmark or rate something, or judge a pair in the arena",
        }),
    }))
}

#[derive(Debug, Deserialize)]
struct ViewQuery {
    #[serde(default = "default_limit")]
    limit: i64,
}

fn default_limit() -> i64 {
    100
}

/// The instance's view of its readership.
async fn operator_view(
    State(state): State<AppState>,
    crate::auth::RequireSession(user): crate::auth::RequireSession,
    Query(query): Query<ViewQuery>,
) -> ApiResult<Json<Value>> {
    crate::routes::experiment_sweep::require_operator(&state, &user)?;
    let rows = darkhold_db::reader_affinity::ranked(state.db(), query.limit)
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?;

    // A distribution, because "the top reader is 0.81" says nothing about whether anybody else is scored. Buckets are
    // deciles of the score itself, and an empty bucket is absent rather than zero.
    let mut buckets = [0usize; 10];
    for row in &rows {
        let bp = row["score_bp"].as_i64().unwrap_or(0).clamp(0, 10_000);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let bucket = (bp as usize / 1_000).min(9);
        buckets[bucket] += 1;
    }
    let distribution: Vec<Value> = buckets
        .iter()
        .enumerate()
        .filter(|(_, count)| **count > 0)
        .map(|(bucket, count)| json!({ "decile": bucket, "readers": count }))
        .collect();

    Ok(Json(json!({
        "readers": rows,
        "distribution": distribution,
        "scored": rows.len(),
    })))
}

/// One sweep.
async fn recompute(
    State(state): State<AppState>,
    crate::auth::RequireSession(user): crate::auth::RequireSession,
) -> ApiResult<Json<Value>> {
    crate::routes::experiment_sweep::require_operator(&state, &user)?;

    // Whose taste the score is measured against. An instance with no designated operator has nothing for a reader to be
    // close to, and the sweep says so rather than scoring everyone against an empty shelf.
    let operator = state
        .config()
        .administration
        .operator_account_id
        .map(|id| id.to_string());

    let now = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();
    let report = darkhold_db::reader_affinity::recompute_all(state.db(), operator.as_deref(), &now)
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?;

    let rows = darkhold_db::reader_affinity::ranked(state.db(), 1_000)
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?;

    let mut body = report.to_json(rows.len());
    if operator.is_none() {
        body["trigger"] = json!("no operator account is configured, so every candidate was reported unscored");
    } else {
        body["trigger"] = json!(
            "operator-triggered: this tree has no scheduler, so the interval in config is an intent and this door is the trigger"
        );
    }
    Ok(Json(body))
}
