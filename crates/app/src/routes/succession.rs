//! §11.11a — succession routes: the document a stranger reads, and the registry behind it (M45-39).
//!
//! Spec: `docs/spec.md` §11.11a. Plan: `docs/plans/m45-39-succession.md`.
//! Store: `crates/db/src/succession.rs`.
//!
//! # The public half is public
//!
//! `GET /api/v1/succession` requires no session. The reader who needs it is holding a dead link,
//! possibly from another instance, and may never have had an account here. A document behind a login
//! answers a question nobody asked at the moment nobody can log in to ask it.
//!
//! # A stale plan is served, not withheld
//!
//! The response carries `confirmed_at`, `review_due` and `stale`. Withholding an unconfirmed plan
//! would make "this instance stopped confirming it" indistinguishable from "this instance never had
//! one", and the first is the fact worth knowing.
//!
//! # The operator half answers 404 to everyone else
//!
//! Same rule as the leakage view and the rest of this tree: an endpoint whose existence is a
//! disclosure must not answer 403.

use axum::extract::{Query, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::auth::{MaybeSession, RequireSession};
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;
use darkhold_domain::error::AppError;
use darkhold_domain::succession::{PlanDraft, DEFAULT_REVIEW_DAYS};

/// Reads are public; writes are the operator's.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/succession", get(read_plan))
        .route("/succession/redirects", get(read_redirect))
        .route("/succession", put(write_plan))
        .route("/succession/confirm", post(confirm_plan))
        .route("/succession", axum::routing::delete(withdraw_plan))
        .route("/succession/redirects", put(write_redirect))
}

/// The public document.
#[derive(Debug, Serialize)]
pub struct SuccessionView {
    /// The plan, or `null` when this instance has published none.
    pub plan: Option<PlanView>,
    /// What this document does not cover, said in the document rather than in a roadmap.
    ///
    /// A reader deciding whether to trust the pointer is told which halves of instance-death
    /// protection are built, so "there is an export URL" is not read as "the archive is mirrored".
    pub not_built: Vec<String>,
}

/// The plan as published.
#[derive(Debug, Serialize)]
pub struct PlanView {
    /// Where readers should go.
    pub destination: String,
    /// Where a full export can be fetched.
    pub export_url: String,
    /// How to reach the operator.
    pub contact: String,
    /// How often it must be re-confirmed, in days.
    pub review_days: i64,
    /// When it was last confirmed, RFC 3339.
    pub confirmed_at: String,
    /// When it stops being current, RFC 3339.
    pub review_due: Option<String>,
    /// Whether it has outlived the window — computed at read time.
    pub stale: bool,
}

/// The body an operator sends to set the plan.
#[derive(Debug, Deserialize)]
pub struct PlanRequest {
    /// Where readers should go.
    pub destination: String,
    /// Where a full export can be fetched.
    pub export_url: String,
    /// How to reach the operator.
    pub contact: String,
    /// How often the plan must be re-confirmed. Defaults to 90.
    #[serde(default)]
    pub review_days: Option<i64>,
}

/// The body an operator sends to re-confirm.
#[derive(Debug, Default, Deserialize)]
pub struct ConfirmRequest {
    /// A new review window, if it changed. Clamped to the §11.11a bounds.
    #[serde(default)]
    pub review_days: Option<i64>,
}

/// One redirect as an operator registers it.
#[derive(Debug, Deserialize)]
pub struct RedirectRequest {
    /// The host the link points at.
    pub host: String,
    /// The path prefix.
    #[serde(default)]
    pub path: Option<String>,
    /// Where it should go now.
    pub to_url: String,
}

/// Query for resolving a link.
#[derive(Debug, Deserialize)]
pub struct ResolveQuery {
    /// The host the dead link points at.
    pub host: String,
    /// The path on that host.
    #[serde(default)]
    pub path: Option<String>,
}

/// What a resolution answers.
#[derive(Debug, Serialize)]
pub struct ResolveView {
    /// Where the link went.
    pub to: String,
}

async fn read_plan(
    State(state): State<AppState>,
    MaybeSession(_user): MaybeSession,
) -> ApiResult<Json<SuccessionView>> {
    let plan = darkhold_db::succession::plan(state.db()).await?;
    let now = time::OffsetDateTime::now_utc();
    Ok(Json(SuccessionView {
        plan: plan.map(|plan| PlanView {
            review_due: plan.review_due(),
            stale: plan.is_stale(now),
            destination: plan.destination,
            export_url: plan.export_url,
            contact: plan.contact,
            review_days: plan.review_days,
            confirmed_at: plan.confirmed_at,
        }),
        not_built: vec![
            "mutual backup with a peer instance".to_owned(),
            "ActivityPub account Move".to_owned(),
        ],
    }))
}

async fn read_redirect(
    State(state): State<AppState>,
    MaybeSession(_user): MaybeSession,
    Query(query): Query<ResolveQuery>,
) -> ApiResult<Json<ResolveView>> {
    let path = query.path.as_deref().unwrap_or("/");
    match darkhold_db::succession::resolve(state.db(), &query.host, path).await? {
        Some(to) => Ok(Json(ResolveView { to })),
        None => Err(ApiError(AppError::NotFound {
            resource: "redirect",
        })),
    }
}

async fn write_plan(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Json(request): Json<PlanRequest>,
) -> ApiResult<Json<SuccessionView>> {
    // `require_admin_of` answers `404` to a caller with no standing at all and `403` to one whose role is real but wrong
    // for this door (ADR 0030), so these doors need no shape check of their own.
    crate::routes::admin_scope::require_admin_of(&state, &user, None).await?;
    let draft = darkhold_domain::succession::validate(PlanDraft {
        destination: request.destination,
        export_url: request.export_url,
        contact: request.contact,
        review_days: request.review_days.unwrap_or(DEFAULT_REVIEW_DAYS),
    })
    .map_err(ApiError)?;
    darkhold_db::succession::set_plan(state.db(), &draft).await?;
    read_plan(State(state), MaybeSession(None)).await
}

async fn confirm_plan(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Json(request): Json<ConfirmRequest>,
) -> ApiResult<Json<SuccessionView>> {
    // `require_admin_of` answers `404` to a caller with no standing at all and `403` to one whose role is real but wrong
    // for this door (ADR 0030), so these doors need no shape check of their own.
    crate::routes::admin_scope::require_admin_of(&state, &user, None).await?;
    let review_days = request.review_days.map(|days| {
        days.clamp(
            darkhold_domain::succession::MIN_REVIEW_DAYS,
            darkhold_domain::succession::MAX_REVIEW_DAYS,
        )
    });
    if !darkhold_db::succession::confirm(state.db(), review_days).await? {
        // Confirming nothing would publish a blank document, which reads as a plan.
        return Err(ApiError(AppError::NotFound {
            resource: "succession plan",
        }));
    }
    read_plan(State(state), MaybeSession(None)).await
}

async fn withdraw_plan(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<SuccessionView>> {
    // `require_admin_of` answers `404` to a caller with no standing at all and `403` to one whose role is real but wrong
    // for this door (ADR 0030), so these doors need no shape check of their own.
    crate::routes::admin_scope::require_admin_of(&state, &user, None).await?;
    darkhold_db::succession::clear(state.db()).await?;
    read_plan(State(state), MaybeSession(None)).await
}

async fn write_redirect(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Json(request): Json<RedirectRequest>,
) -> ApiResult<Json<ResolveView>> {
    // `require_admin_of` answers `404` to a caller with no standing at all and `403` to one whose role is real but wrong
    // for this door (ADR 0030), so these doors need no shape check of their own.
    crate::routes::admin_scope::require_admin_of(&state, &user, None).await?;
    if !request.to_url.starts_with("https://") {
        return Err(ApiError(AppError::field(
            "to_url",
            "A redirect target must be https.",
        )));
    }
    let path = request.path.as_deref().unwrap_or("/");
    let registered =
        darkhold_db::succession::add_redirect(state.db(), &request.host, path, &request.to_url)
            .await?;
    if !registered {
        return Err(ApiError(AppError::field(
            "host",
            "That host is not a host this registry can key on.",
        )));
    }
    Ok(Json(ResolveView { to: request.to_url }))
}

// The local `require_operator` is gone: succession re-points an identity, which is estate state rather than this community's
// content, so it uses the cross-site door (ADR 0003). Replacing it also removes a fourth definition of "operator" — the
// config-only one, which disagreed with the trust-based one and would have let the two drift apart unnoticed.
