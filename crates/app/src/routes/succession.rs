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

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
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
        .route("/admin/succession/redirects", get(list_redirects))
        .route("/admin/succession/mirrors", put(write_mirror))
        .route(
            "/admin/succession/mirrors/{peer}",
            axum::routing::delete(withdraw_mirror),
        )
}

/// The public document.
#[derive(Debug, Serialize)]
pub struct SuccessionView {
    /// The plan, or `null` when this instance has published none.
    pub plan: Option<PlanView>,
    /// The peers an operator has asserted hold a copy, ordered by peer id.
    pub mirrors: Vec<MirrorView>,
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

/// One peer holding a copy, as the public document states it.
#[derive(Debug, Serialize)]
pub struct MirrorView {
    /// The peer, by the id its fingerprint declares.
    pub peer_id: String,
    /// Where its copy of the export can be fetched.
    pub export_url: String,
    /// When the operator asserted it holds that copy, RFC 3339.
    pub asserted_at: String,
    /// Always false: this instance records an assertion and cannot reach into a peer's disk.
    ///
    /// The field exists so a reader is not invited to read "a copy exists" as "a copy was checked". It is a constant
    /// rather than an absent field because silence reads as a yes.
    pub verified: bool,
}

/// The body an operator sends to record a peer.
#[derive(Debug, Deserialize)]
pub struct MirrorRequest {
    /// The peer, by the id its fingerprint declares.
    pub peer_id: String,
    /// Where its copy of this instance's export can be fetched. `https://` only.
    pub export_url: String,
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
    let mirrors = darkhold_db::succession::mirrors(state.db()).await?;
    let has_mirror = !mirrors.is_empty();
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
        mirrors: mirrors
            .into_iter()
            .map(|mirror| MirrorView {
                peer_id: mirror.peer_id,
                export_url: mirror.export_url,
                asserted_at: mirror.asserted_at,
                verified: false,
            })
            .collect(),
        // Computed from the rows, never hardcoded: an instance that has recorded a mirror must stop confessing that the
        // half is missing, and one whose operator withdrew it must start again. `ActivityPub account Move` stays
        // unconditional — darkhold federates inbound only, so that half is not this instance's to build.
        not_built: {
            let mut missing = Vec::new();
            if !has_mirror {
                missing.push("mutual backup with a peer instance".to_owned());
            }
            missing.push("ActivityPub account Move".to_owned());
            missing
        },
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

/// What one host's redirect registry says, in resolution order.
#[derive(Debug, Serialize)]
pub struct RedirectListView {
    /// The normalised host that was asked about.
    pub host: String,
    /// Longest prefix first — the order the public door resolves in, so the listing answers "which rule wins" rather than
    /// implying by alphabet that a short prefix catches a longer one.
    pub redirects: Vec<RedirectRow>,
}

/// One published redirect.
#[derive(Debug, Serialize)]
pub struct RedirectRow {
    /// The path prefix.
    pub path: String,
    /// Where a reader following it is sent.
    pub to_url: String,
}

/// `?host=` for the registry listing.
#[derive(Debug, Deserialize)]
pub struct HostQuery {
    /// The host whose redirects are being listed.
    pub host: String,
}

/// The registry an operator has published for one host.
///
/// `PUT /succession/redirects` could write redirects and `GET /succession/redirects?path=` could resolve one guessed path;
/// `succession::redirects_for`, the query that lists them, had no caller. So an operator could publish a registry and never
/// read it back: a duplicate, a typo'd host, or a target that has moved again stayed invisible until a reader hit the 404.
/// **A registry nobody can list is a registry nobody can audit**, and this is the door that closes that.
///
/// Operator-only, unlike the public resolve door beside it. A single path with a known host is a reader's question; every
/// host and path this instance has ever answered for is instance internals, and enumerating it is not something a stranger
/// should be able to do one harmless query at a time.
async fn list_redirects(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Query(query): Query<HostQuery>,
) -> ApiResult<Json<RedirectListView>> {
    crate::routes::admin_scope::require_admin_of(&state, &user, None).await?;
    let redirects = darkhold_db::succession::redirects_for(state.db(), &query.host).await?;
    Ok(Json(RedirectListView {
        host: darkhold_domain::succession::normalise_host(&query.host).unwrap_or(query.host),
        redirects: redirects
            .into_iter()
            .map(|(path, to_url)| RedirectRow { path, to_url })
            .collect(),
    }))
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

/// Record the assertion that a peer holds a copy.
async fn write_mirror(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Json(request): Json<MirrorRequest>,
) -> ApiResult<(StatusCode, Json<SuccessionView>)> {
    // `require_admin_of` answers `404` to a caller with no standing at all and `403` to one whose role is real but wrong
    // for this door (ADR 0030), so these doors need no shape check of their own.
    crate::routes::admin_scope::require_admin_of(&state, &user, None).await?;
    let peer = request.peer_id.trim();
    if peer.is_empty() || peer.len() > 64 || !peer
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
    {
        // A peer id is a name another instance publishes, not free text: empty means "no peer", and the characters are the
        // ones an instance id is spelled with. Refused by field so the operator knows which half to fix.
        return Err(ApiError(AppError::field(
            "peer_id",
            "A peer id is 1..=64 characters of letters, digits, '-', '_', '.' or ':'.",
        )));
    }
    if !request.export_url.starts_with("https://") {
        return Err(ApiError(AppError::field(
            "export_url",
            "A mirror's export must be https.",
        )));
    }
    darkhold_db::succession::set_mirror(state.db(), peer, &request.export_url).await?;
    let view = read_plan(State(state), MaybeSession(None)).await?;
    Ok((StatusCode::CREATED, view))
}

/// Withdraw one peer's assertion.
async fn withdraw_mirror(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(peer): Path<String>,
) -> ApiResult<StatusCode> {
    // `require_admin_of` answers `404` to a caller with no standing at all and `403` to one whose role is real but wrong
    // for this door (ADR 0030), so these doors need no shape check of their own.
    crate::routes::admin_scope::require_admin_of(&state, &user, None).await?;
    if !darkhold_db::succession::remove_mirror(state.db(), &peer).await? {
        // Withdrawing an assertion nobody made is not "already gone": it is a request about a peer this instance never
        // recorded, and `404` says exactly that instead of implying the operator's work was done.
        return Err(ApiError(AppError::NotFound { resource: "mirror" }));
    }
    Ok(StatusCode::NO_CONTENT)
}

// The local `require_operator` is gone: succession re-points an identity, which is estate state rather than this community's
// content, so it uses the cross-site door (ADR 0003). Replacing it also removes a fourth definition of "operator" — the
// config-only one, which disagreed with the trust-based one and would have let the two drift apart unnoticed.
