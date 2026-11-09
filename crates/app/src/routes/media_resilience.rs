use crate::auth::{MaybeSession, RequirePseud, RequireSession};
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use darkhold_db::media_resilience;
use darkhold_domain::media_resilience::{LinkStatus, MediaContextKind};
use darkhold_domain::AppError;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

fn link_to_json(row: &media_resilience::AvailabilityLinkRow) -> Value {
    json!({
        "id": row.id,
        "url": row.url,
        "provider": row.provider,
        "status": row.status,
        "priority": row.priority,
        "last_checked_at": row.last_checked_at,
        "last_healthy_at": row.last_healthy_at,
        "consecutive_failures": row.consecutive_failures,
    })
}

fn reference_to_json(
    row: &media_resilience::MediaReference,
    healthy_links: i64,
    links: &[media_resilience::AvailabilityLinkRow],
) -> Value {
    json!({
        "id": row.id,
        "media_kind": row.media_kind,
        "content_hash": row.content_hash,
        "perceptual_hash": row.perceptual_hash,
        "curator_verified": row.curator_verified,
        "healthy_links": healthy_links,
        "links": links.iter().map(link_to_json).collect::<Vec<_>>(),
    })
}

// ---------------------------------------------------------------------------
// Public routes
// ---------------------------------------------------------------------------

/// Get a media reference with its availability links.
pub async fn get_media_reference(
    State(state): State<AppState>,
    MaybeSession(_session): MaybeSession,
    Path(reference_id): Path<String>,
) -> ApiResult<Json<Value>> {
    // A segment that is not a UUID cannot name a row, and on PostgreSQL trying
    // anyway is a database error rather than a miss. Answer 404 without asking.
    if !darkhold_db::is_uuid(&reference_id) {
        return Err(ApiError(AppError::NotFound {
            resource: "media_reference",
        }));
    }

    let Some(reference) = media_resilience::find_media_reference_by_id(state.db(), &reference_id)
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?
    else {
        return Err(ApiError(AppError::NotFound {
            resource: "media_reference",
        }));
    };

    let links = media_resilience::find_availability_links_for_reference(state.db(), &reference.id)
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?;

    let healthy = media_resilience::count_healthy_links(state.db(), &reference.id)
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?;

    Ok(Json(reference_to_json(&reference, healthy, &links)))
}

/// Get media references for a work.
pub async fn get_work_media_references(
    State(state): State<AppState>,
    MaybeSession(_session): MaybeSession,
    Path(work_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let refs = media_resilience::list_work_media_references(state.db(), &work_id)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;

    Ok(Json(json!({ "items": refs })))
}

// ---------------------------------------------------------------------------
// Authenticated routes
// ---------------------------------------------------------------------------

/// Body for adding a media reference to a work.
#[derive(Debug, Deserialize)]
pub struct AddMediaReferenceBody {
    pub url: String,
    pub context: Option<String>,
    pub author_note: Option<String>,
    pub unit_id: Option<String>,
}

pub async fn add_media_reference(
    State(state): State<AppState>,
    RequirePseud { user, pseud_id }: RequirePseud,
    Path(work_id): Path<String>,
    Json(body): Json<AddMediaReferenceBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    // Verify user can edit the work (owner, contributor, or operator).
    let account_id = user.account_id.to_string();
    let level = darkhold_db::governance::trust_for(state.db(), &account_id)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    let is_operator = level >= 5;
    let can_edit = is_operator
        || media_resilience::can_edit_work(
            state.db(),
            &work_id,
            &account_id,
            &pseud_id.to_string(),
        )
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    if !can_edit {
        return Err(ApiError(AppError::AuthRequired));
    }
    let context: MediaContextKind = body
        .context
        .as_deref()
        .and_then(|c| c.parse().ok())
        .unwrap_or(MediaContextKind::Reference);

    let link_id = Uuid::new_v4().to_string();
    let reference_id = Uuid::new_v4().to_string();
    let account_id = user.account_id.to_string();

    media_resilience::insert_media_reference(
        state.db(),
        &reference_id,
        "pending", // content hash computed async by the pipeline
        darkhold_domain::media_resilience::MediaKind::Image,
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e)))?;

    media_resilience::insert_availability_link(
        state.db(),
        &link_id,
        &reference_id,
        &body.url,
        darkhold_domain::media_resilience::LinkProvider::Other,
        Some(&account_id),
        100,
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e)))?;

    media_resilience::insert_work_media_reference(
        state.db(),
        &Uuid::new_v4().to_string(),
        &work_id,
        body.unit_id.as_deref(),
        &reference_id,
        context,
        &body.url,
        body.author_note.as_deref(),
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e)))?;

    // The hashing happens in the worker, not here: the URL is author-supplied
    // and the fetch must not hold the author's request open. The payload names
    // only the reference, so a job row never carries the URL, and a corrected
    // link is picked up by a retry.
    let job_id = crate::media_job::enqueue_media_fetch(state.db(), &reference_id)
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?;

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "id": reference_id,
            "fingerprint_job_id": job_id,
            "link_id": link_id,
            "status": "pending_verification"
        })),
    ))
}

/// Report a broken link.
#[derive(Debug, Deserialize)]
pub struct ReportBrokenBody {
    pub reason: Option<String>,
}

pub async fn report_broken_link(
    State(state): State<AppState>,
    RequireSession(_user): RequireSession,
    Path(link_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let links = media_resilience::find_links_needing_check(state.db(), 10000)
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?;
    let Some(link) = links.iter().find(|l| l.id == link_id) else {
        return Err(ApiError(AppError::NotFound {
            resource: "availability_link",
        }));
    };

    // Mark link as degraded pending verification
    media_resilience::update_link_status(
        state.db(),
        &link_id,
        LinkStatus::PendingVerification,
        link.consecutive_failures + 1,
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e)))?;

    Ok(Json(json!({ "status": "reported" })))
}

// ---------------------------------------------------------------------------
// Curator routes
// ---------------------------------------------------------------------------

/// Body for adding an availability link (curator action).
#[derive(Debug, Deserialize)]
pub struct AddMirrorBody {
    pub url: String,
    pub provider: Option<String>,
    pub priority: Option<i64>,
}

pub async fn add_mirror_link(
    State(state): State<AppState>,
    RequirePseud { user, .. }: RequirePseud,
    Path(reference_id): Path<String>,
    Json(body): Json<AddMirrorBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let account_id = user.account_id.to_string();
    // Check curator role.
    let is_operator = {
        let level = darkhold_db::governance::trust_for(state.db(), &account_id)
            .await
            .map_err(|e| ApiError(AppError::Internal(e.into())))?;
        level >= 5
    };
    if !is_operator
        && !media_resilience::is_active_curator(state.db(), &account_id)
            .await
            .map_err(|e| ApiError(AppError::Internal(e.into())))?
    {
        return Err(ApiError(AppError::AuthRequired));
    }
    let Some(_reference) = media_resilience::find_media_reference_by_id(state.db(), &reference_id)
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?
    else {
        return Err(ApiError(AppError::NotFound {
            resource: "media_reference",
        }));
    };

    let provider: darkhold_domain::media_resilience::LinkProvider = body
        .provider
        .as_deref()
        .and_then(|p| p.parse().ok())
        .unwrap_or(darkhold_domain::media_resilience::LinkProvider::Other);

    let priority = body.priority.unwrap_or(50);

    let link_id = Uuid::new_v4().to_string();
    let account_id = user.account_id.to_string();

    media_resilience::insert_availability_link(
        state.db(),
        &link_id,
        &reference_id,
        &body.url,
        provider,
        Some(&account_id),
        priority,
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e)))?;

    // **The daily cap is the half of the bounty rule that was never applied.** The config declared
    // `media_resilience.daily_credits_cap` (500), `caps::within_daily_cap` was a tested predicate, and
    // `sum_curator_rewards_today` was a query — and nothing in the award path read any of the three, so the same mirror added
    // a hundred times paid a hundred times. A cap of `0` or less means "no cap" rather than "nobody earns": a limit that
    // silently zeroes the feature is a worse default than no limit, and 500 is what the config ships.
    let cap = state.config().media_resilience.daily_credits_cap;
    let bounty = state.config().media_resilience.mirror_add_credits;
    let awarded = if cap <= 0 {
        bounty
    } else {
        let earned = media_resilience::sum_curator_rewards_today(state.db(), &account_id)
            .await
            .map_err(|e| ApiError(AppError::Internal(e)))?;
        if !darkhold_domain::caps::within_daily_cap(earned, cap) {
            0
        } else {
            bounty.min(cap - earned)
        }
    };

    // The link is kept whether or not it pays: the correction is worth having after the cap, and refusing the contribution
    // to enforce an accounting rule would be the wrong trade. The response says which happened, so a curator who is not
    // being paid can see that instead of guessing.
    if awarded > 0 {
        media_resilience::insert_curator_reward(
            state.db(),
            &account_id,
            darkhold_domain::media_resilience::CuratorAction::MirrorAdd,
            Some(&reference_id),
            Some(&link_id),
            awarded,
        )
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?;
    }

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "id": link_id,
            "status": "pending_verification",
            "credits_awarded": awarded,
            "credits_capped": awarded < bounty,
        })),
    ))
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/media/references/{reference_id}", get(get_media_reference))
        .route(
            "/media/references/{reference_id}/report-broken",
            post(report_broken_link),
        )
        .route(
            "/works/{work_id}/media",
            get(get_work_media_references).post(add_media_reference),
        )
        .route(
            "/media/references/{reference_id}/mirrors",
            post(add_mirror_link),
        )
        .route("/admin/notes/links/sweep", post(sweep_note_links))
        .route("/admin/media/references/at-risk", get(at_risk_references))
        .route("/media/match-proposals", get(list_match_proposals))
        .route(
            "/media/match-proposals/{proposal_id}",
            get(get_match_proposal).post(resolve_match_proposal),
        )
}

/// The references that need another mirror, which is what the bounties are for.
///
/// `find_references_below_threshold` had no caller, so the queue the curator bounties are paid against did not exist: a
/// curator could only find an under-mirrored reference by stumbling into it. The threshold is the spec's own number (a
/// reference with fewer than three healthy links is under-mirrored) and is a parameter because the honest answer to "how
/// many is enough" changes with the archive, not with the code.
#[derive(Debug, Deserialize)]
struct AtRiskQuery {
    #[serde(default)]
    threshold: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
}

async fn at_risk_references(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Query(query): Query<AtRiskQuery>,
) -> ApiResult<Json<Value>> {
    require_operator(&state, &user).await?;
    let threshold = query.threshold.unwrap_or(3).clamp(0, 50);
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    let rows = media_resilience::find_references_below_threshold(state.db(), threshold, limit)
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?;
    Ok(Json(json!({
        "threshold": threshold,
        "items": rows,
    })))
}

/// One match proposal, with the reason it was resolved the way it was.
///
/// The list route answers "what is waiting"; this answers "what happened to the one I asked about", which is the only
/// record of a rejected merge — a merge that was proposed and refused leaves no other trace.
async fn get_match_proposal(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(proposal_id): Path<String>,
) -> ApiResult<Json<Value>> {
    // Both refusals are the same answer: a proposal id is a capability, not a public identifier, and a malformed one reads
    // exactly like somebody else's.
    require_operator(&state, &user).await?;
    let Some(proposal) = media_resilience::find_match_proposal(state.db(), &proposal_id)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?
    else {
        return Err(ApiError(AppError::NotFound {
            resource: "match_proposal",
        }));
    };
    let body = json!({
        "id": proposal.id,
        "candidate_reference_id": proposal.candidate_reference_id,
        "existing_reference_id": proposal.existing_reference_id,
        "content_hash": proposal.content_hash,
        "perceptual_hash": proposal.perceptual_hash,
        "hamming_distance": proposal.hamming_distance,
        "match_confidence": proposal.match_confidence,
        "status": proposal.status,
        "resolved_by": proposal.resolved_by,
        "resolution_note": proposal.resolution_note,
        "created_at": proposal.created_at,
        "updated_at": proposal.updated_at,
        "resolved_at": proposal.resolved_at,
    });
    Ok(Json(body))
}

#[derive(Debug, Deserialize)]
pub struct SweepBody {
    /// How many links to check. Each one is an outbound request with an 8-second timeout, so this is bounded rather than
    /// "all of them": an operator asking for a sweep should get an answer, not a request that runs past the client.
    #[serde(default)]
    pub limit: Option<i64>,
    /// Re-check only links nobody has looked at yet, which is the cheap first pass on a fresh instance.
    #[serde(default)]
    pub never_checked_only: bool,
}

/// Check the note links the monitor has looked at least recently.
///
/// **`find_needing_check` had no caller, which made the monitor a per-work button rather than a monitor.** A link was
/// checked only when its author pressed "re-check" on the work that carried it: a dead link on an abandoned work was
/// never noticed, and the `dead` count on that work's page was a number that could not change on its own.
///
/// There is no scheduler in this binary (§37 puts recurring work on the operator), so this is a **button, not a timer** —
/// the scheduled form is named as the follow-up rather than pretended. The queue is ordered never-checked first, then
/// oldest, and the store's query has no "due" predicate: a sweep will happily re-check a link checked a minute ago. That
/// is why `limit` exists and why the answer reports what it actually did.
///
/// A link that cannot be checked is recorded as a failure by [`check_note_link`] itself — a timeout is the answer, not an
/// error — so a sweep never fails because one URL is gone. Only a storage failure aborts.
async fn sweep_note_links(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Json(body): Json<SweepBody>,
) -> ApiResult<Json<Value>> {
    require_operator(&state, &user).await?;
    let limit = body.limit.unwrap_or(20).clamp(1, 50);
    let queued = darkhold_db::note_links::find_needing_check(state.db(), limit * 4)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
    let selected: Vec<_> = queued
        .into_iter()
        .filter(|link| !body.never_checked_only || link.last_checked_at.is_none())
        .take(limit as usize)
        .collect();

    let mut healthy = 0usize;
    let mut degraded = 0usize;
    let mut dead = 0usize;
    let mut failed = 0usize;
    for link in &selected {
        match crate::note_link_job::check_note_link(&state, &link.id, None).await {
            Ok(LinkStatus::Healthy) => healthy += 1,
            Ok(LinkStatus::Degraded) => degraded += 1,
            Ok(LinkStatus::Dead) => dead += 1,
            // `check_note_link` returns a `HandlerError` only for a storage or lookup failure: the URL's answer is the
            // recorded status. Counted rather than propagated, because one unreadable row must not stop the sweep, and
            // reported rather than swallowed, because a sweep that silently skipped rows would look complete.
            Ok(_) => failed += 1,
            Err(error) => {
                tracing::warn!(link_id = %link.id, %error, "a note link could not be swept");
                failed += 1;
            }
        }
    }

    Ok(Json(json!({
        "checked": selected.len(),
        "healthy": healthy,
        "degraded": degraded,
        "dead": dead,
        "failed": failed,
        "limit": limit,
    })))
}

/// Gate for the curator-facing media endpoints.
///
/// Trust level 5 rather than the media opt-in table, because these merge and
/// discard media references: that is a moderation action, and the opt-in list
/// exists for curators who want *extra* work, not a lower bar for destructive
/// ones. Mirrors `media_health::require_operator` so the two agree on who may.
async fn require_operator(
    state: &AppState,
    user: &crate::auth::SessionUser,
) -> Result<(), ApiError> {
    let level = darkhold_db::governance::trust_for(state.db(), &user.account_id.to_string())
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    if level >= 5 {
        Ok(())
    } else {
        // The caller *is* signed in -- `RequireSession` already proved that --
        // so "authentication required" is a lie that also hides the real reason
        // from the client. A reader hitting this gets 403 and can act on it.
        Err(ApiError(AppError::AccessDenied))
    }
}

// ---------------------------------------------------------------------------
// §32.7.2 Perceptual match proposals (curator review)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ResolveProposalBody {
    /// `confirm` merges the candidate into the existing reference; `reject`
    /// keeps them apart. Anything else is a 422 rather than a silent default,
    /// because the two outcomes are opposites and guessing one is destructive.
    pub decision: String,
    #[serde(default)]
    pub note: Option<String>,
}

fn proposal_to_json(row: &media_resilience::MatchProposal) -> Value {
    json!({
        "id": row.id,
        "candidate_reference_id": row.candidate_reference_id,
        "existing_reference_id": row.existing_reference_id,
        "content_hash": row.content_hash,
        "perceptual_hash": row.perceptual_hash,
        "hamming_distance": row.hamming_distance,
        "match_confidence": row.match_confidence,
        "status": row.status,
        "created_at": row.created_at,
    })
}

/// The pending proposals, closest match first.
///
/// Bounded by a `limit` that the caller controls up to 200, because a fetch
/// storm on one popular image produces a proposal per importing work and an
/// unbounded queue is the failure this endpoint exists to prevent. The total
/// pending count is returned alongside it, so a curator can see that there is
/// more than they are being shown.
pub async fn list_match_proposals(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Query(query): Query<MatchProposalQuery>,
) -> ApiResult<Json<Value>> {
    require_operator(&state, &user).await?;

    let limit = query.limit.unwrap_or(50);
    let pending = media_resilience::list_pending_match_proposals(state.db(), limit)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    let total = media_resilience::count_pending_match_proposals(state.db())
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;

    Ok(Json(json!({
        "pending": pending
            .iter()
            .map(|p| json!({
                "proposal": proposal_to_json(&p.proposal),
                "existing_content_hash": p.existing_content_hash,
            }))
            .collect::<Vec<_>>(),
        "total_pending": total,
    })))
}

#[derive(Debug, Default, Deserialize)]
pub struct MatchProposalQuery {
    pub limit: Option<i64>,
}

/// Confirm or reject one proposal.
pub async fn resolve_match_proposal(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(proposal_id): Path<String>,
    Json(body): Json<ResolveProposalBody>,
) -> ApiResult<Json<Value>> {
    require_operator(&state, &user).await?;

    let decision = match body.decision.as_str() {
        "confirm" => media_resilience::ProposalDecision::Confirm,
        "reject" => media_resilience::ProposalDecision::Reject,
        other => {
            return Err(ApiError(AppError::Validation {
                message: format!("decision must be `confirm` or `reject`, got `{other}`"),
                field_errors: std::collections::BTreeMap::new(),
            }));
        }
    };
    let note = body
        .note
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty());

    let resolved = media_resilience::resolve_match_proposal(
        state.db(),
        &proposal_id,
        decision,
        &user.account_id.to_string(),
        note,
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e.into())))?;

    if !resolved {
        // Missing, already decided, or not an id at all -- one 404 for all three.
        // Distinguishing them would tell a curator that someone else got there
        // first, which is not their business while they are still deciding.
        return Err(ApiError(AppError::NotFound {
            resource: "match_proposal",
        }));
    }
    Ok(Json(json!({
        "id": proposal_id,
        "status": decision.as_status(),
        "merged": decision.is_confirmation(),
    })))
}
