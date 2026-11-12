//! M45-45 step 3, the door — a lens change as a proposal the instance votes on.
//!
//! ```text
//! POST /operator/taste-proposals              open a proposal that carries a whole lens (operator only)
//! GET  /taste-proposals/{id}                  the proposal, its state and the votes cast so far
//! POST /taste-proposals/{id}/vote             cast or change one vote (any signed-in reader, one per account)
//! ```
//!
//! # Why the open door is operator-only and the vote door is not
//!
//! Under `[taste] lens_owner = "vote"` a direct write to the lens is refused, so *somebody* has to be able to put a
//! question to the instance: that is the operator's act, and it is the only one here that decides what will be asked.
//! Voting is every reader's, because the point of the mode is that the lens is not one person's.
//!
//! # Why a closed proposal is decided by a read
//!
//! `decide_if_due` runs when a proposal is read. There is no scheduler in this door and no timer: a proposal nobody reads
//! is a proposal nobody is waiting on, and the applying write is a transaction either way. The alternative — a background
//! sweep — would decide the same proposal in two places, and the two would disagree about `decided_at` eventually.
//!
//! # Why the vote is not anonymous
//!
//! A vote is one per account, and the primary key on `(proposal_id, voter_account)` is what enforces it. An anonymous
//! vote could not be one per anything.

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::RequireSession;
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;
use darkhold_db::instance_taste_settings::TasteSettingsUpdate;
use darkhold_domain::AppError;

/// How far ahead a proposal closes when the caller does not say. Seven days, because a lens is not a referendum and a
/// proposal that closes in an hour is decided by whoever happens to be awake.
const DEFAULT_WINDOW_DAYS: i64 = 7;

/// The share of cast votes a proposal needs by default.
const DEFAULT_THRESHOLD_PERCENT: i64 = 60;

/// The taste-proposal routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/taste-proposals", post(open_proposal))
        .route("/taste-proposals/{id}", get(read_proposal))
        .route("/taste-proposals/{id}/vote", post(cast_vote))
}

#[derive(Debug, Deserialize)]
struct OpenRequest {
    gravity_strength: i64,
    signal_weight_mode: String,
    admin_weight: i64,
    diversity_injection_percent: i64,
    /// Days until the proposal closes. Absent means `DEFAULT_WINDOW_DAYS`.
    #[serde(default)]
    closes_in_days: Option<i64>,
    /// The share of cast votes needed to pass. Absent means `DEFAULT_THRESHOLD_PERCENT`.
    #[serde(default)]
    threshold_percent: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct VoteRequest {
    /// `yes` or `no`.
    vote: String,
}

fn proposal_json(proposal: &darkhold_db::taste_lens_proposals::Proposal) -> Value {
    json!({
        "id": proposal.id,
        "gravity_strength": proposal.gravity_strength,
        "signal_weight_mode": proposal.signal_weight_mode,
        "admin_weight": proposal.admin_weight,
        "diversity_injection_percent": proposal.diversity_injection_percent,
        "opened_by": proposal.opened_by,
        "opened_at": proposal.opened_at,
        "closes_at": proposal.closes_at,
        "threshold_percent": proposal.threshold_percent,
        "state": proposal.state,
        "decided_at": proposal.decided_at,
    })
}

/// Open a proposal. Operator only: it decides what the instance will be asked.
async fn open_proposal(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Json(body): Json<OpenRequest>,
) -> ApiResult<Json<Value>> {
    crate::routes::experiment_sweep::require_operator(&state, &user)?;

    let update = TasteSettingsUpdate {
        gravity_strength: body.gravity_strength,
        signal_weight_mode: body.signal_weight_mode,
        admin_weight: body.admin_weight,
        diversity_injection_percent: body.diversity_injection_percent,
    };
    let days = body.closes_in_days.unwrap_or(DEFAULT_WINDOW_DAYS);
    if days <= 0 {
        return Err(ApiError(AppError::field(
            "closes_in_days",
            "a proposal that closes today is not a vote: name a window of at least one day",
        )));
    }
    let threshold = body.threshold_percent.unwrap_or(DEFAULT_THRESHOLD_PERCENT);
    if !(1..=100).contains(&threshold) {
        return Err(ApiError(AppError::field(
            "threshold_percent",
            "a threshold is a percentage of cast votes: 1 to 100",
        )));
    }

    let closes_at = closes_at(days);
    let proposal = darkhold_db::taste_lens_proposals::open(
        state.db(),
        &update,
        &user.account_id.to_string(),
        &closes_at,
        threshold,
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e)))?;

    Ok(Json(json!({
        "proposal": proposal_json(&proposal),
        "tally": { "yes": 0, "no": 0 },
    })))
}

/// Read a proposal, deciding it first if it has closed.
async fn read_proposal(
    State(state): State<AppState>,
    RequireSession(_user): RequireSession,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let now = darkhold_db::identity::now_rfc3339();
    let proposal = darkhold_db::taste_lens_proposals::decide_if_due(state.db(), &id, &now)
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?
        .ok_or_else(|| {
            ApiError(AppError::NotFound {
                resource: "proposal",
            })
        })?;
    let tally = darkhold_db::taste_lens_proposals::tally(state.db(), &id)
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?;

    Ok(Json(json!({
        "proposal": proposal_json(&proposal),
        "tally": { "yes": tally.yes, "no": tally.no, "yes_percent": tally.yes_percent() },
    })))
}

/// Cast or change one vote. A second vote from the same account replaces the first.
async fn cast_vote(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
    Json(body): Json<VoteRequest>,
) -> ApiResult<Json<Value>> {
    let yes = match body.vote.as_str() {
        "yes" => true,
        "no" => false,
        other => {
            return Err(ApiError(AppError::field(
                "vote",
                format!("`{other}` is not a vote: say `yes` or `no`"),
            )))
        }
    };

    let proposal = darkhold_db::taste_lens_proposals::get(state.db(), &id)
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?
        .ok_or_else(|| {
            ApiError(AppError::NotFound {
                resource: "proposal",
            })
        })?;
    if proposal.state != "open" {
        // A vote on a decided proposal would be recorded against a decision it cannot change; refusing by name is what
        // makes the client's mistake fixable.
        return Err(ApiError(AppError::field(
            "proposal",
            format!(
                "this proposal is `{}` and no longer takes votes",
                proposal.state
            ),
        )));
    }

    let tally = darkhold_db::taste_lens_proposals::cast_vote(
        state.db(),
        &id,
        &user.account_id.to_string(),
        yes,
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e)))?;

    Ok(Json(json!({
        "proposal_id": id,
        "tally": { "yes": tally.yes, "no": tally.no, "yes_percent": tally.yes_percent() },
    })))
}

/// RFC 3339 in UTC, `days` ahead of now — the format every writer of this column uses, so the string comparison in
/// `decide_if_due` is a time comparison.
fn closes_at(days: i64) -> String {
    let now = time::OffsetDateTime::now_utc();
    (now + time::Duration::days(days))
        .format(&time::format_description::well_known::Rfc3339)
        .expect("an RFC 3339 timestamp formats")
}
