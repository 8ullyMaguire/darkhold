//! M45-33 — specialist matchmaking over HTTP.
//!
//! ```text
//! GET/PUT/DELETE  /specialists/me                     a profile, written by its owner
//! POST            /works/:id/specialist-requests      ask for a specialist on your work
//! GET             /works/:id/specialist-requests      what has been asked for on it
//! GET             /specialist-requests?skill=         what a specialist could take
//! POST            /specialist-requests/:id/accept     take one
//! POST            /specialist-requests/:id/complete   hand the work back
//! POST            /specialist-requests/:id/withdraw   take it back, or step away
//! ```
//!
//! # What the routes hold that the store does not
//!
//! * **A profile is written by the pseud that owns it.** There is no route that writes someone else's profile,
//!   and no path parameter naming a pseud: the session decides whose row this is.
//! * **A request can only be opened on a work you contribute to**, checked through the contributor table the
//!   collaborators module owns rather than by comparing `owner_pseud_id` inline — a work can have co-authors,
//!   and "the owner" is the wrong test for them.
//! * **Accepting is a compare-and-set.** The store's `WHERE state = 'open'` decides the winner; this module
//!   turns its `false` into a 409 rather than a silent 200, because a specialist who was told "accepted"
//!   about a request somebody else is holding would start editing a work that will never reach them.
//! * **Critique only where requested.** There is no route by which a specialist writes anything: completing
//!   records that the work was returned, and nothing in this module carries a comment.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use darkhold_db::specialist as store;
use darkhold_domain::specialist::{self, Capacity, Skill, State as RequestState};

use crate::auth::{RequireSession, SessionUser};
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;
use darkhold_domain::error::AppError;

/// The specialist routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/specialists/me", get(get_me).put(put_me).delete(delete_me))
        .route(
            "/works/{id}/specialist-requests",
            get(requests_for_work).post(create_request),
        )
        .route("/specialist-requests", get(open_requests))
        .route("/specialist-requests/{id}/accept", post(accept))
        .route("/specialist-requests/{id}/complete", post(complete))
        .route("/specialist-requests/{id}/withdraw", post(withdraw))
}

fn invalid(message: impl Into<String>) -> ApiError {
    ApiError(AppError::Validation {
        message: message.into(),
        field_errors: std::collections::BTreeMap::new(),
    })
}

fn acting_pseud(user: &SessionUser) -> ApiResult<String> {
    user.pseud_id
        .map(|pseud| pseud.to_string())
        .ok_or(ApiError(AppError::AccessDenied))
}

#[derive(Debug, Deserialize)]
struct ProfileBody {
    /// `beta`, `britpick`, `canon`, `sensitivity`.
    skills: Vec<String>,
    /// Fandom names this person will take. Empty means any.
    #[serde(default)]
    fandoms: Vec<String>,
    /// `open` (default) or `closed`.
    capacity: Option<String>,
    #[serde(default)]
    note: String,
}

fn profile_json(profile: &store::Profile, summary: Value) -> Value {
    json!({
        "pseud_id": profile.pseud_id,
        "skills": profile.skills.iter().map(|skill| skill.as_str()).collect::<Vec<_>>(),
        "fandoms": profile.fandoms,
        "capacity": profile.capacity.as_str(),
        "note": profile.note,
        "updated_at": profile.updated_at,
        "candidates": summary,
    })
}

async fn get_me(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<Value>> {
    let pseud = acting_pseud(&user)?;
    let profile = store::profile(state.db(), &pseud).await?;
    Ok(Json(
        json!({ "profile": profile.as_ref().map(|row| profile_json(row, Value::Null)) }),
    ))
}

async fn put_me(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Json(body): Json<ProfileBody>,
) -> ApiResult<Json<Value>> {
    let pseud = acting_pseud(&user)?;
    if body.skills.is_empty() {
        return Err(invalid(
            "skills: name at least one of beta, britpick, canon, sensitivity — an empty profile is not an opt-in",
        ));
    }
    let mut skills = Vec::new();
    for raw in &body.skills {
        let skill = Skill::parse(raw).ok_or_else(|| {
            invalid(format!(
                "skills: {raw} is not one of beta, britpick, canon, sensitivity"
            ))
        })?;
        if !skills.contains(&skill) {
            skills.push(skill);
        }
    }
    let capacity = match body.capacity.as_deref() {
        None => Capacity::Open,
        Some(raw) => Capacity::parse(raw)
            .ok_or_else(|| invalid(format!("capacity: {raw} is not open or closed")))?,
    };
    let profile = store::upsert_profile(
        state.db(),
        &pseud,
        &skills,
        &body.fandoms,
        capacity,
        body.note.trim(),
    )
    .await?;
    Ok(Json(profile_json(&profile, Value::Null)))
}

async fn delete_me(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<StatusCode> {
    let pseud = acting_pseud(&user)?;
    store::delete_profile(state.db(), &pseud).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct RequestBody {
    skill: String,
    #[serde(default)]
    fandom: String,
    #[serde(default)]
    brief: String,
}

async fn create_request(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(work): Path<String>,
    Json(body): Json<RequestBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let pseud = acting_pseud(&user)?;
    let skill = Skill::parse(&body.skill).ok_or_else(|| {
        invalid(format!(
            "skill: {} is not one of beta, britpick, canon, sensitivity",
            body.skill
        ))
    })?;

    // The work must be one this pseud contributes to: co-authors may ask for a specialist too.
    let work_id = work
        .parse::<darkhold_domain::ids::WorkId>()
        .map_err(|_| ApiError(AppError::NotFound { resource: "work" }))?;
    let exists = darkhold_db::content::find_work(state.db(), work_id)
        .await?
        .is_some();
    let contributors = darkhold_db::collaboration::contributors_for_work(state.db(), work_id)
        .await
        .unwrap_or_default();
    let mine = contributors
        .iter()
        .any(|row| row.pseud_id.to_string() == pseud);
    if !exists || !mine {
        return Err(ApiError(AppError::NotFound { resource: "work" }));
    }

    let id = uuid::Uuid::new_v4().to_string();
    let request = store::create_request(
        state.db(),
        &id,
        &work,
        &pseud,
        skill,
        body.fandom.trim(),
        body.brief.trim(),
    )
    .await?;

    // The matching list travels with the request, so the interface can show it without a second call.
    let candidates = store::candidates(state.db(), skill).await?;
    let ranked = specialist::rank(&request.fandom, candidates);
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "request": request_json(&request),
            "matches": matches_json(&request.fandom, &ranked),
        })),
    ))
}

fn request_json(request: &store::Request) -> Value {
    json!({
        "id": request.id,
        "work_id": request.work_id,
        "skill": request.skill.as_str(),
        "fandom": request.fandom,
        "brief": request.brief,
        "state": request.state.as_str(),
        "matched_pseud_id": request.matched_pseud_id,
        "created_at": request.created_at,
        "matched_at": request.matched_at,
        "completed_at": request.completed_at,
    })
}

fn matches_json(fandom: &str, ranked: &[specialist::Ranked]) -> Value {
    json!({
        "summary": specialist::summary(fandom, ranked),
        "items": ranked.iter().map(|row| json!({
            "pseud_id": row.candidate.pseud_id,
            "fit": row.fit,
            "live_engagements": row.candidate.live_engagements,
            "completed": row.candidate.completed,
        })).collect::<Vec<_>>(),
    })
}

async fn requests_for_work(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(work): Path<String>,
) -> ApiResult<Json<Value>> {
    let _ = acting_pseud(&user)?;
    let rows = store::requests_for_work(state.db(), &work).await?;
    Ok(Json(json!({
        "items": rows.iter().map(request_json).collect::<Vec<_>>(),
    })))
}

#[derive(Debug, Deserialize)]
struct BrowseParams {
    /// `beta`, `britpick`, `canon` or `sensitivity`; required, because an unfiltered feed is not a task list.
    skill: Option<String>,
    limit: Option<usize>,
}

async fn open_requests(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Query(params): Query<BrowseParams>,
) -> ApiResult<Json<Value>> {
    let _ = acting_pseud(&user)?;
    let skill = params
        .skill
        .as_deref()
        .ok_or_else(|| invalid("skill: name the skill you can take"))?;
    let skill = Skill::parse(skill).ok_or_else(|| {
        invalid(format!(
            "skill: {skill} is not one of beta, britpick, canon, sensitivity"
        ))
    })?;
    let limit = params.limit.unwrap_or(50).clamp(1, 200);
    let rows = store::open_requests(state.db(), skill).await?;
    Ok(Json(json!({
        "items": rows.iter().take(limit).map(request_json).collect::<Vec<_>>(),
    })))
}

async fn accept(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let pseud = acting_pseud(&user)?;
    transition(
        &state,
        &id,
        RequestState::Open,
        RequestState::Matched,
        Some(&pseud),
    )
    .await
}

async fn complete(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let _ = acting_pseud(&user)?;
    transition(
        &state,
        &id,
        RequestState::Matched,
        RequestState::Completed,
        None,
    )
    .await
}

async fn withdraw(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let _ = acting_pseud(&user)?;
    let request = store::request(state.db(), &id).await?.ok_or_else(|| {
        ApiError(AppError::NotFound {
            resource: "specialist request",
        })
    })?;
    // Withdrawing is legal from either live state; the row's own state decides which.
    let from = match request.state {
        RequestState::Open | RequestState::Matched => request.state,
        other => {
            return Err(ApiError(AppError::Validation {
                message: format!(
                    "cannot withdraw a request that is {} — it is already finished",
                    other.as_str()
                ),
                field_errors: std::collections::BTreeMap::new(),
            }))
        }
    };
    transition(&state, &id, from, RequestState::Withdrawn, None).await
}

async fn transition(
    state: &AppState,
    id: &str,
    from: RequestState,
    to: RequestState,
    matched: Option<&str>,
) -> ApiResult<Json<Value>> {
    // The domain decides whether the move is legal at all; the store decides who won the race.
    let legal = specialist::transition(
        from,
        match to {
            RequestState::Matched => specialist::Action::Accept,
            RequestState::Completed => specialist::Action::Complete,
            _ => specialist::Action::Withdraw,
        },
    );
    if let Err(refusal) = legal {
        return Err(ApiError(AppError::Validation {
            message: refusal.0,
            field_errors: std::collections::BTreeMap::new(),
        }));
    }
    if !store::set_state(state.db(), id, from, to, matched).await? {
        return Err(ApiError(AppError::Validation {
            message: format!(
                "this request is no longer {} — somebody else moved it first",
                from.as_str()
            ),
            field_errors: std::collections::BTreeMap::new(),
        }));
    }
    let request = store::request(state.db(), id).await?.ok_or_else(|| {
        ApiError(AppError::NotFound {
            resource: "specialist request",
        })
    })?;
    Ok(Json(request_json(&request)))
}
