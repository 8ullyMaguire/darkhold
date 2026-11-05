//! M45-34 — bounties over HTTP.
//!
//! ```text
//! POST /finder/bounties                        open one (session, credits reserved)
//! GET  /finder/bounties?fandom=                open ones (public)
//! GET  /finder/bounties/:id                    one, with its answers (public)
//! POST /finder/bounties/:id/proposals          answer it (session, not the asker)
//! POST /finder/bounties/:id/accept             pay an answer (session, the asker)
//! POST /finder/bounties/:id/cancel             withdraw it (session, the asker)
//! GET  /finder/me                              what I asked and what I answered (session)
//! ```
//!
//! Reads are public because a bounty is a public question — an offer nobody can see is answered by nobody.
//! Writes are session-scoped, and the two that move credits (accept, cancel) are asker-only.
//!
//! The acceptance route takes the bounty id even though the proposal id is enough, so a proposal cannot be paid
//! against a different bounty than the one the caller is looking at: a mismatched pair is refused.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use darkhold_db::finder::{self as store, Accepted, Created, NewBounty};

use crate::auth::{MaybeSession, RequireSession};
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;
use darkhold_domain::error::AppError;

/// The bounty routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/finder/bounties", get(list).post(create))
        .route("/finder/bounties/{id}", get(one))
        .route("/finder/bounties/{id}/proposals", post(propose))
        .route("/finder/bounties/{id}/accept", post(accept))
        .route("/finder/bounties/{id}/cancel", post(cancel))
        .route("/finder/me", get(mine))
}

fn invalid(message: impl Into<String>) -> ApiError {
    ApiError(AppError::Validation {
        message: message.into(),
        field_errors: std::collections::BTreeMap::new(),
    })
}

fn bounty_json(row: &commons_bounty::Bounty) -> Value {
    json!({
        "id": row.id,
        "asker_account_id": row.asker_account_id,
        "description": row.description,
        "fandom": row.subject,
        "budget": row.budget,
        "state": row.state.as_str(),
        "closes_at": row.closes_at,
        "created_at": row.created_at,
        "accepted_proposal_id": row.accepted_proposal_id,
    })
}

fn proposal_json(row: &commons_bounty::Proposal) -> Value {
    json!({
        "id": row.id,
        "bounty_id": row.bounty_id,
        "proposer_account_id": row.proposer_account_id,
        "work_id": row.subject_id,
        "note": row.note,
        "state": row.state,
        "created_at": row.created_at,
    })
}

#[derive(Debug, Deserialize)]
struct NewBountyBody {
    description: String,
    fandom: Option<String>,
    budget: i64,
    closes_at: String,
}

async fn create(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Json(body): Json<NewBountyBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let new = NewBounty {
        description: body.description,
        fandom: body
            .fandom
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty()),
        budget: body.budget,
        closes_at: body.closes_at,
    };
    match store::create(state.db(), &user.account_id.to_string(), &new).await? {
        Created::Bounty(bounty) => Ok((
            StatusCode::CREATED,
            Json(json!({ "bounty": bounty_json(&bounty) })),
        )),
        Created::Refused(reason) => Err(invalid(reason)),
    }
}

#[derive(Debug, Deserialize)]
struct ListParams {
    fandom: Option<String>,
}

async fn list(
    State(state): State<AppState>,
    MaybeSession(_session): MaybeSession,
    Query(params): Query<ListParams>,
) -> ApiResult<Json<Value>> {
    let rows = store::open_bounties(state.db(), params.fandom.as_deref()).await?;
    Ok(Json(json!({
        "items": rows.iter().map(bounty_json).collect::<Vec<_>>(),
        "count": rows.len(),
    })))
}

async fn one(
    State(state): State<AppState>,
    MaybeSession(_session): MaybeSession,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let Some(bounty) = store::get(state.db(), &id).await? else {
        return Err(ApiError(AppError::NotFound { resource: "bounty" }));
    };
    let answers = store::proposals(state.db(), &id).await?;
    Ok(Json(json!({
        "bounty": bounty_json(&bounty),
        "proposals": answers.iter().map(proposal_json).collect::<Vec<_>>(),
    })))
}

#[derive(Debug, Deserialize)]
struct ProposeBody {
    work_id: String,
    #[serde(default)]
    note: String,
}

async fn propose(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
    Json(body): Json<ProposeBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    match store::propose(
        state.db(),
        &id,
        &user.account_id.to_string(),
        &body.work_id,
        &body.note,
    )
    .await?
    {
        Ok(proposal) => Ok((
            StatusCode::CREATED,
            Json(json!({ "proposal": proposal_json(&proposal) })),
        )),
        // Refusals here are either "no such bounty" or a rule; both are answered with the sentence rather than a
        // bare status, because "you cannot answer your own bounty" is the useful part.
        Err(reason) => Err(invalid(reason)),
    }
}

#[derive(Debug, Deserialize)]
struct AcceptBody {
    proposal_id: String,
}

async fn accept(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
    Json(body): Json<AcceptBody>,
) -> ApiResult<Json<Value>> {
    match store::accept(
        state.db(),
        &id,
        &body.proposal_id,
        &user.account_id.to_string(),
    )
    .await?
    {
        Accepted::Paid {
            bounty,
            transaction_id,
        } => Ok(Json(json!({
            "bounty": bounty_json(&bounty),
            "transaction_id": transaction_id,
            "paid": bounty.budget,
        }))),
        Accepted::Refused(reason) => Err(invalid(reason)),
    }
}

async fn cancel(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    match store::cancel(state.db(), &id, &user.account_id.to_string()).await? {
        Some(bounty) => Ok(Json(json!({ "bounty": bounty_json(&bounty) }))),
        None => Err(ApiError(AppError::NotFound {
            resource: "open bounty of yours",
        })),
    }
}

async fn mine(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<Value>> {
    let account = user.account_id.to_string();
    let available = store::available_credits(state.db(), &account).await?;
    let open = store::open_bounties(state.db(), None).await?;
    let mine: Vec<Value> = open
        .iter()
        .filter(|row| row.asker_account_id == account)
        .map(bounty_json)
        .collect();
    Ok(Json(json!({
        "available_credits": available,
        "open_bounties_i_asked": mine,
    })))
}
