//! `GET|POST /claims` — claiming works that arrived as metadata (M45-42).
//!
//! The proof is a token in a page the author already controls, so the refusals here are about *what the token proves*:
//!
//! 1. **A page without the token proves nothing** — `422` naming `profile_url`, and the claim stays `pending`. It is not a
//!    refusal of the author, it is a visit that established nothing, and the difference decides whether they try again.
//! 2. **A work already owned is refused by name** — `already_claimed`, in the per-work list. `claimed: 0` with no reasons
//!    reads as "nothing to do".
//! 3. **The exclusion registry outranks the claim** — an author who asked to be left out cannot be pulled back in through a
//!    door that bypasses the registry.
//! 4. **A claim id is not a capability** — a stranger gets `404`.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::auth::RequireSession;
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;
use darkhold_domain::error::AppError;

/// This module's routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/claims", post(create_claim))
        .route("/claims/{id}", get(read_claim))
        .route("/claims/{id}/verify", post(verify_claim))
}

/// The body of a claim request.
#[derive(Debug, Deserialize)]
pub struct ClaimRequest {
    /// The works the author says are theirs.
    #[serde(default)]
    pub works: Vec<String>,
    /// The profile page the token will be placed on.
    pub profile_url: String,
}

/// What a claim answers with on creation.
#[derive(Debug, Serialize)]
pub struct ClaimCreated {
    /// The claim's id.
    pub claim_id: String,
    /// The string to paste into the profile.
    pub token: String,
    /// The page it must appear on.
    pub profile_url: String,
    /// What the author has to do, in one sentence, so the token is not a riddle.
    pub instructions: String,
}

/// One work's fate under a claim.
#[derive(Debug, Serialize)]
pub struct RefusedWork {
    /// The work.
    pub work: String,
    /// Why it was not attached.
    pub reason: String,
}

/// What verification answered.
#[derive(Debug, Serialize)]
pub struct VerifyOutcome {
    /// How many works were attached by this call.
    pub claimed: usize,
    /// The works that were not, and why.
    pub refused: Vec<RefusedWork>,
}

/// `POST /claims` — mint the token, or return the one already minted.
async fn create_claim(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Json(request): Json<ClaimRequest>,
) -> ApiResult<(StatusCode, Json<ClaimCreated>)> {
    let url = request.profile_url.trim();
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(ApiError(AppError::field(
            "profile_url",
            "A claim's profile must be an http(s) URL.",
        )));
    }
    if request.works.is_empty() {
        // An empty claim cannot attach anything and asks the author to paste a token for no reason.
        return Err(ApiError(AppError::field(
            "works",
            "Name at least one work to claim.",
        )));
    }
    let account = user.account_id.to_string();
    let works: Vec<String> = request
        .works
        .iter()
        .map(|work| work.trim().to_owned())
        .filter(|work| !work.is_empty())
        .collect();
    let claim = darkhold_db::claims::create_or_get(state.db(), &account, url, &works)
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?;
    Ok((
        StatusCode::CREATED,
        Json(ClaimCreated {
            claim_id: claim.id,
            token: claim.token.clone(),
            profile_url: claim.profile_url.clone(),
            instructions: format!(
                "Put this line in your profile bio, then verify: {}",
                claim.token
            ),
        }),
    ))
}

/// `GET /claims/{id}` — this claim's state and what it decided per work.
async fn read_claim(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let account = user.account_id.to_string();
    let claim = darkhold_db::claims::find_owned(state.db(), &id, &account)
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?;
    let Some(claim) = claim else {
        // A claim id is not a capability: same answer as a claim that never existed.
        return Err(ApiError(AppError::NotFound { resource: "claim" }));
    };
    let works = darkhold_db::claims::works_of(state.db(), &claim.id)
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?;
    Ok(Json(serde_json::json!({
        "claim_id": claim.id,
        "state": claim.state,
        "profile_url": claim.profile_url,
        "works": works,
    })))
}

/// `POST /claims/{id}/verify` — look for the token, then attach what is attachable.
async fn verify_claim(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
) -> ApiResult<Json<VerifyOutcome>> {
    let account = user.account_id.to_string();
    let claim = darkhold_db::claims::find_owned(state.db(), &id, &account)
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?;
    let Some(claim) = claim else {
        return Err(ApiError(AppError::NotFound { resource: "claim" }));
    };

    let page = state
        .profile_fetcher()
        .fetch(&claim.profile_url)
        .await
        .map_err(|_| {
            // No page, no proof — and no refusal either: AO3 being slow is not this author's fault, so the claim stays
            // `pending` and the answer says where to look.
            ApiError(AppError::field(
                "profile_url",
                "the page could not be fetched",
            ))
        })?;
    if !page.contains(&claim.token) {
        return Err(ApiError(AppError::field(
            "profile_url",
            "the token was not found on that page",
        )));
    }

    let pseud = darkhold_db::claims::primary_pseud(state.db(), &account)
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?;
    let Some(pseud) = pseud else {
        return Err(ApiError(AppError::field(
            "works",
            "This account has no pseud to attach works to.",
        )));
    };

    let works = darkhold_db::claims::works_of(state.db(), &claim.id)
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?;
    let mut claimed = 0usize;
    let mut refused = Vec::new();
    for decision in works {
        // Only a metadata row is claimable. A work somebody already owns is not this author's to take, whatever the page
        // says — and saying which work and why is the difference between a refusal and a silent no-op.
        let metadata = darkhold_db::claims::is_metadata_work(state.db(), &decision.work)
            .await
            .map_err(|e| ApiError(AppError::Internal(e)))?;
        if !metadata {
            darkhold_db::claims::set_outcome(
                state.db(),
                &claim.id,
                &decision.work,
                "already_claimed",
            )
            .await
            .map_err(|e| ApiError(AppError::Internal(e)))?;
            refused.push(RefusedWork {
                work: decision.work,
                reason: "already_claimed".to_owned(),
            });
            continue;
        }
        let excluded = darkhold_db::permission::is_excluded(
            state.db(),
            darkhold_db::permission::ExclusionTarget::Work,
            &decision.work,
        )
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?;
        if excluded {
            darkhold_db::claims::set_outcome(
                state.db(),
                &claim.id,
                &decision.work,
                "excluded_by_author",
            )
            .await
            .map_err(|e| ApiError(AppError::Internal(e)))?;
            refused.push(RefusedWork {
                work: decision.work,
                reason: "excluded_by_author".to_owned(),
            });
            continue;
        }
        darkhold_db::claims::attach(state.db(), &decision.work, &pseud)
            .await
            .map_err(|e| ApiError(AppError::Internal(e)))?;
        darkhold_db::claims::set_outcome(state.db(), &claim.id, &decision.work, "claimed")
            .await
            .map_err(|e| ApiError(AppError::Internal(e)))?;
        claimed += 1;
    }
    if claimed > 0 {
        darkhold_db::claims::mark_verified(state.db(), &claim.id)
            .await
            .map_err(|e| ApiError(AppError::Internal(e)))?;
    }
    Ok(Json(VerifyOutcome { claimed, refused }))
}
