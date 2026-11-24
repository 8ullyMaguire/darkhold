//! Community trope proposal doors (M45-77 Step 9).
//!
//! Public reads return a coarse view: tallies, state and what the group would
//! expand to. Account ids and individual votes never leave this module.

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::{MaybeSession, RequireSession};
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

use darkhold_db::trope_governance as store;
use darkhold_domain::error::AppError;
use darkhold_domain::trope_governance::{valid_slug, TropeAction};

#[derive(Debug, Deserialize)]
pub struct CreateProposal {
    pub slug: String,
    pub action: String,
    pub label: String,
    pub summary: String,
    pub members: Vec<String>,
    pub target_slug: Option<String>,
    pub target_version: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct VoteBody {
    pub value: String,
}

#[derive(Debug, Deserialize)]
pub struct SettleBody {
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    100
}

fn role_name(role: Option<crate::trope_authority::TropeRole>) -> Option<&'static str> {
    use crate::trope_authority::TropeRole;
    match role {
        Some(TropeRole::Curator) => Some("curator"),
        Some(TropeRole::Trusted) => Some("trusted"),
        Some(TropeRole::Operator) => Some("operator"),
        None => None,
    }
}

fn public_proposal(row: &store::TropeProposal) -> ApiResult<Value> {
    let payload =
        store::parse_payload(&row.payload).map_err(|e| ApiError(AppError::Internal(e.into())))?;
    Ok(json!({
        "id": row.id,
        "slug": row.slug,
        "action": row.action,
        "status": row.status,
        "yes_votes": row.yes_votes,
        "no_votes": row.no_votes,
        "quorum_needed": row.quorum_needed,
        "closes_at": row.closes_at,
        "created_at": row.created_at,
        "decided_at": row.decided_at,
        "proposer_role": payload.proposer_role,
        "expands_to": payload.members,
    }))
}

fn internal(e: anyhow::Error) -> ApiError {
    ApiError(AppError::Internal(e))
}

fn now_and_closes(days: i64) -> ApiResult<(String, String)> {
    let now = chrono::Utc::now();
    let closes = now + chrono::Duration::days(days);
    let opts = chrono::SecondsFormat::Secs;
    Ok((
        now.to_rfc3339_opts(opts, true),
        closes.to_rfc3339_opts(opts, true),
    ))
}

pub async fn create_proposal(
    RequireSession(user): RequireSession,
    State(state): State<AppState>,
    Json(body): Json<CreateProposal>,
) -> ApiResult<(axum::http::StatusCode, Json<Value>)> {
    let db = state.db();
    let account = user.account_id.to_string();
    let cfg = &state.config().tropes;
    let authority = crate::trope_authority::for_account(db, cfg, &account)
        .await
        .map_err(|e| internal(e.into()))?;
    if !authority.allowed {
        return Err(ApiError(AppError::TrustLevelInsufficient {
            level: authority.level,
            required: cfg.proposal_min_trust,
        }));
    }
    let action = TropeAction::parse(&body.action).ok_or_else(|| {
        ApiError(AppError::field(
            "action",
            "a trope proposal is create, update or retire",
        ))
    })?;
    if !valid_slug(&body.slug) || darkhold_domain::tropes::slugs().contains(&body.slug.as_str()) {
        return Err(ApiError(AppError::field(
            "slug",
            format!(
                "{} is not a new 3..40 character lowercase hyphenated slug; seed groups cannot be proposed",
                body.slug
            ),
        )));
    }
    if body.members.is_empty() || body.members.len() > 10 {
        return Err(ApiError(AppError::field(
            "members",
            "a trope group needs 1..10 members",
        )));
    }
    let mut missing = Vec::new();
    for tag in &body.members {
        if !darkhold_db::taxonomy::tag_exists(db, tag)
            .await
            .map_err(internal)?
        {
            missing.push(tag.clone());
        }
    }
    if !missing.is_empty() {
        return Err(ApiError(AppError::field(
            "members",
            format!("unknown tags: {}", missing.join(", ")),
        )));
    }
    let target_slug = body.target_slug.clone();
    let target_version = body.target_version.clone();
    if action != TropeAction::Create {
        let target = target_slug.as_deref().unwrap_or("");
        if !valid_slug(target) {
            return Err(ApiError(AppError::field(
                "target_slug",
                "update and retire need a valid target slug",
            )));
        }
        let active = store::active_group_details(db)
            .await
            .map_err(internal)?
            .into_iter()
            .find(|group| group.slug == target);
        let current = active.ok_or_else(|| {
            ApiError(AppError::field(
                "target_slug",
                "no active community group has that slug",
            ))
        })?;
        if target_version.as_deref() != Some(current.version.as_str()) {
            return Err(ApiError(AppError::field(
                "target_version",
                "the target version does not match the active group",
            )));
        }
    }
    // The unique live-slug index allows one open proposal per group. An update
    // or retirement is a *different* proposal whose payload names the target;
    // reusing the group slug would collide with the active group itself.
    let proposal_slug = if action == TropeAction::Create {
        body.slug.clone()
    } else {
        let target = target_slug.as_deref().expect("checked above");
        format!("{}-{target}", action.as_str())
    };
    let (now, closes_at) = now_and_closes(cfg.voting_days)?;
    let payload = json!({
        "label": body.label,
        "summary": body.summary,
        "members": body.members,
        "target_slug": target_slug,
        "target_version": target_version,
        "proposer_role": role_name(authority.role),
    })
    .to_string();
    let id = store::insert_proposal(
        db,
        &proposal_slug,
        action.as_str(),
        &payload,
        cfg.quorum as i64,
        &closes_at,
        &account,
        &now,
    )
    .await
    .map_err(internal)?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(json!({
            "id": id,
            "status": "proposed",
            "quorum_needed": cfg.quorum,
        })),
    ))
}

pub async fn list_proposals(
    State(state): State<AppState>,
    MaybeSession(_user): MaybeSession,
) -> ApiResult<Json<Value>> {
    let rows = store::list_proposals(state.db(), None, 100)
        .await
        .map_err(internal)?;
    let mut items = Vec::new();
    for row in &rows {
        items.push(public_proposal(row)?);
    }
    Ok(Json(json!({ "items": items })))
}

pub async fn get_proposal(
    State(state): State<AppState>,
    MaybeSession(_user): MaybeSession,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let row = store::get_proposal(state.db(), &id)
        .await
        .map_err(internal)?
        .ok_or(ApiError(AppError::NotFound {
            resource: "trope proposal",
        }))?;
    Ok(Json(public_proposal(&row)?))
}

pub async fn vote(
    RequireSession(user): RequireSession,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<VoteBody>,
) -> ApiResult<Json<Value>> {
    if body.value != "yes" && body.value != "no" {
        return Err(ApiError(AppError::field(
            "value",
            "a trope vote is yes or no",
        )));
    }
    let db = state.db();
    let account = user.account_id.to_string();
    let cfg = &state.config().tropes;
    let authority = crate::trope_authority::for_account(db, cfg, &account)
        .await
        .map_err(|e| internal(e.into()))?;
    if !authority.allowed {
        return Err(ApiError(AppError::TrustLevelInsufficient {
            level: authority.level,
            required: cfg.proposal_min_trust,
        }));
    }
    let proposal = store::get_proposal(db, &id)
        .await
        .map_err(internal)?
        .ok_or(ApiError(AppError::NotFound {
            resource: "trope proposal",
        }))?;
    if proposal.proposed_by == account {
        // The proposer may not count as an independent approval.
        return Err(ApiError(AppError::AccessDenied));
    }
    let now = now_and_closes(0)?.0;
    let counted = store::insert_vote(db, &id, &account, &body.value, &now)
        .await
        .map_err(internal)?;
    let decided = store::recount_and_decide(db, &id, &now, &account)
        .await
        .map_err(internal)?;
    Ok(Json(json!({
        "proposal_id": id,
        "changed": counted,
        "counted": counted,
        "decided": decided,
    })))
}

pub async fn settle(
    RequireSession(user): RequireSession,
    State(state): State<AppState>,
    Json(body): Json<SettleBody>,
) -> ApiResult<Json<store::SettleReport>> {
    crate::routes::discovery::require_operator(&state, &user)?;
    let now = now_and_closes(0)?.0;
    let report = store::settle_expired(state.db(), body.limit, &now)
        .await
        .map_err(internal)?;
    Ok(Json(report))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/tropes/proposals",
            post(create_proposal).get(list_proposals),
        )
        .route("/tropes/proposals/{id}", get(get_proposal))
        .route("/tropes/proposals/{id}/vote", post(vote))
        .route("/operator/tropes/settle", post(settle))
}
