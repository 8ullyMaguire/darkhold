//! §21.6's gallery over HTTP (M16-08): install, uninstall, update, rate, and read a package's score.
//!
//! ```text
//! POST   /api/v1/extensions/{slug}/install       install (idempotent), scope = the acting pseud
//! DELETE /api/v1/extensions/{slug}/install       uninstall
//! POST   /api/v1/extensions/{slug}/update        apply the installation's update policy
//! PUT    /api/v1/extensions/{slug}/rating        rate, and write a review if there is one
//! GET    /api/v1/extensions/{slug}/ratings       the score and the written reviews
//! GET    /api/v1/me/installations               the caller's own installations
//! ```
//!
//! # Where the scope comes from
//!
//! Every installation and rating belongs to a **pseud**, taken from the acting session's active pseud — never from the request body. A
//! client-supplied scope would let one account install or rate as another identity, which is the one thing a per-scope rule is for.
//!
//! # What the routes refuse to decide
//!
//! Which versions a policy allows, whether a rating is a rating, and whether a fork's licence is compatible all live in
//! `darkhold_domain::gallery`. These handlers read a package, ask the domain module, and store the answer.

use axum::extract::{Path, Query, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::RequireSession;
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;
use darkhold_domain::extension::Manifest;
use darkhold_domain::gallery::{
    compatible, GalleryRefusal, InstallOutcome, Rating, UpdatePolicy,
};
use darkhold_domain::AppError;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/extensions/{slug}/install",
            post(install).delete(uninstall),
        )
        .route("/extensions/{slug}/update", post(update))
        // `PUT` only: the read is `/ratings`, so one path means one thing. A GET here as well would be a second spelling of the same read,
        // and the route inventory (correctly) refuses to have two doors where it tabled one.
        .route("/extensions/{slug}/rating", put(rate))
        .route("/extensions/{slug}/ratings", get(read_ratings))
        .route("/me/installations", get(my_installations))
}

/// The pseud a gallery row belongs to.
///
/// A session may have no pseud selected (a fresh sign-in before the reader picks an identity), and a gallery row with no scope would have to
/// belong to the account — which is the thing "per pseud scope" rules out. Refused by name so the client can send the reader to pick one.
fn acting_scope(user: &crate::auth::SessionUser) -> ApiResult<String> {
    user.pseud_id
        .as_ref()
        .map(ToString::to_string)
        .ok_or_else(|| {
            ApiError(AppError::Validation {
                message: "gallery actions happen as a pseud; select one first".to_owned(),
                field_errors: Default::default(),
            })
        })
}

fn refused(refusal: GalleryRefusal) -> ApiError {
    // A gallery refusal is a rule about this package, so it is a 409 with the rule's sentence. A `403` would say the caller is not allowed
    // to install anything, which is untrue and would send them to check their session.
    ApiError(AppError::Validation {
        message: refusal.message(),
        field_errors: Default::default(),
    })
}

/// The published manifest of a package, or a 404 naming that nothing is installable.
async fn published(state: &AppState, slug: &str) -> ApiResult<Manifest> {
    let document = darkhold_db::marketplace::manifest_document_for_latest(state.db(), slug)
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?
        .ok_or_else(|| {
            ApiError(AppError::Validation {
                message: GalleryRefusal::NotPublished(slug.to_owned()).message(),
                field_errors: Default::default(),
            })
        })?;
    serde_json::from_str::<Manifest>(&document)
        .map_err(|error| ApiError(AppError::Internal(anyhow::Error::from(error))))
}

#[derive(Debug, Deserialize)]
pub struct InstallBody {
    /// `pinned` (default), `minor` or `any`.
    #[serde(default)]
    pub update_policy: Option<String>,
    /// An exact version. Refused if it is not a published version of this package.
    #[serde(default)]
    pub version: Option<String>,
}

async fn install(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(slug): Path<String>,
    Json(body): Json<InstallBody>,
) -> ApiResult<Json<Value>> {
    let manifest = published(&state, &slug).await?;
    let scope = acting_scope(&user)?;
    let policy = match body.update_policy.as_deref() {
        None => UpdatePolicy::default(),
        Some(text) => UpdatePolicy::from_str(text).ok_or_else(|| {
            ApiError(AppError::field(
                "update_policy",
                "an update policy is `pinned`, `minor` or `any`",
            ))
        })?,
    };

    let published_versions = darkhold_db::gallery::published_versions(state.db(), &slug)
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?;
    let version = match body.version.as_deref() {
        Some(exact) => {
            if !published_versions.iter().any(|candidate| candidate == exact) {
                return Err(refused(GalleryRefusal::NotPublished(format!(
                    "{slug} {exact}"
                ))));
            }
            exact.to_owned()
        }
        None => published_versions
            .first()
            .cloned()
            .unwrap_or_else(|| manifest.version.to_string()),
    };

    // Installing is consenting: the grant is written before the installation so a package cannot appear installed to a reader whose
    // capabilities were never granted. A grant that already covers these permissions is an upsert, not a second consent.
    let capabilities = manifest
        .permissions
        .iter()
        .map(|capability| capability.as_str().to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    if !capabilities.is_empty() {
        darkhold_db::marketplace::grant_extension(
            state.db(),
            &user.account_id.to_string(),
            &slug,
            &version,
            &manifest.permissions,
        )
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?;
    }

    let (outcome, installation) =
        darkhold_db::gallery::install(state.db(), &scope, &slug, &version, policy)
            .await
            .map_err(|error| ApiError(AppError::Internal(error.into())))?;
    let count = darkhold_db::gallery::install_count(state.db(), &slug)
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?;

    Ok(Json(json!({
        "installed": matches!(outcome, InstallOutcome::Installed),
        // Said explicitly because "installing again" is the case a client retries into, and the answer is the same installation either way.
        "already_installed": matches!(outcome, InstallOutcome::AlreadyInstalled),
        "manifest_id": installation.manifest_id,
        "version": installation.version,
        "update_policy": installation.policy.as_str(),
        "install_count": count,
    })))
}

async fn uninstall(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(slug): Path<String>,
) -> ApiResult<Json<Value>> {
    let scope = acting_scope(&user)?;
    let removed = darkhold_db::gallery::uninstall(state.db(), &scope, &slug)
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?;
    let count = darkhold_db::gallery::install_count(state.db(), &slug)
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?;
    Ok(Json(json!({
        "uninstalled": removed,
        "install_count": count,
    })))
}

#[derive(Debug, Deserialize)]
pub struct UpdateBody {
    #[serde(default)]
    pub update_policy: Option<String>,
}

async fn update(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(slug): Path<String>,
    Json(body): Json<UpdateBody>,
) -> ApiResult<Json<Value>> {
    let scope = acting_scope(&user)?;
    let installation = darkhold_db::gallery::active_installation(state.db(), &scope, &slug)
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?
        .ok_or_else(|| refused(GalleryRefusal::NotInstalled))?;

    // A caller may change the policy as part of an update — "stop following minor releases" is a thing a reader says once, at the moment
    // they notice a release.
    let policy = match body.update_policy.as_deref() {
        None => installation.policy,
        Some(text) => UpdatePolicy::from_str(text).ok_or_else(|| {
            ApiError(AppError::field(
                "update_policy",
                "an update policy is `pinned`, `minor` or `any`",
            ))
        })?,
    };

    let published_versions = darkhold_db::gallery::published_versions(state.db(), &slug)
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?;
    let Some(target) = policy.newest_allowed(&installation.version, &published_versions) else {
        return Ok(Json(json!({
            "updated": false,
            "version": installation.version,
            "update_policy": policy.as_str(),
            "reason": "no release this policy allows",
        })));
    };

    // An update that widens the permission set is a new consent, not an automatic move: the reader granted what they granted, and
    // §21.5's renewal clause exists because silently widening it is how a package acquires a permission nobody agreed to.
    let granted = darkhold_db::marketplace::granted_capabilities(
        state.db(),
        &user.account_id.to_string(),
        &slug,
    )
    .await
    .map_err(|error| ApiError(AppError::Internal(error.into())))?;
    let requested: std::collections::BTreeSet<String> =
        match darkhold_db::marketplace::manifest_document(state.db(), &slug, &target)
            .await
            .map_err(|error| ApiError(AppError::Internal(error.into())))?
        {
            Some(document) => serde_json::from_str::<Manifest>(&document)
                .map(|manifest| {
                    manifest
                        .permissions
                        .iter()
                        .map(|capability| capability.as_str().to_owned())
                        .collect()
                })
                .unwrap_or_default(),
            None => Default::default(),
        };
    if darkhold_domain::extension_review::Review::needs_renewed_consent(&granted, &requested) {
        let new = darkhold_domain::extension_review::Review::new_capabilities(&granted, &requested)
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        return Err(refused(GalleryRefusal::ConsentRequired(new)));
    }

    darkhold_db::gallery::set_version(state.db(), &scope, &slug, &target, policy)
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?;
    Ok(Json(json!({
        "updated": true,
        "from": installation.version,
        "version": target,
        "update_policy": policy.as_str(),
    })))
}

#[derive(Debug, Deserialize)]
pub struct RatingBody {
    pub stars: i64,
    #[serde(default)]
    pub review: Option<String>,
}

async fn rate(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(slug): Path<String>,
    Json(body): Json<RatingBody>,
) -> ApiResult<Json<Value>> {
    let scope = acting_scope(&user)?;
    let installation = darkhold_db::gallery::active_installation(state.db(), &scope, &slug)
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?
        // A rating is an opinion about having used the thing. Without an installation there is nothing to have used, and a gallery where
        // anyone can rate anything is a popularity contest rather than a review.
        .ok_or_else(|| refused(GalleryRefusal::NotInstalled))?;
    let rating = Rating::validate(body.stars, body.review.as_deref()).map_err(refused)?;

    darkhold_db::gallery::rate(
        state.db(),
        &scope,
        &slug,
        &installation.version,
        rating.stars,
        rating.review.as_deref(),
    )
    .await
    .map_err(|error| ApiError(AppError::Internal(error.into())))?;

    let score = darkhold_db::gallery::score(state.db(), &slug)
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?;
    Ok(Json(json!({
        "stars": rating.stars,
        "version_rated": installation.version,
        "score": score.render(),
        "count": score.count,
    })))
}

#[derive(Debug, Deserialize)]
pub struct RatingsQuery {
    #[serde(default)]
    pub limit: Option<i64>,
}

async fn read_ratings(
    State(state): State<AppState>,
    // Declared `Authenticated` in the route inventory, so the door has to ask for a session: it publishes a package's install count, and a
    // door whose row says authenticated while its signature admits anyone is the disagreement the inventory exists to catch.
    RequireSession(_user): RequireSession,
    Path(slug): Path<String>,
    Query(query): Query<RatingsQuery>,
) -> ApiResult<Json<Value>> {
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    let score = darkhold_db::gallery::score(state.db(), &slug)
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?;
    let reviews = darkhold_db::gallery::reviews(state.db(), &slug, limit)
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?;
    let install_count = darkhold_db::gallery::install_count(state.db(), &slug)
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?;
    Ok(Json(json!({
        "score": score.render(),
        "average": score.average,
        "count": score.count,
        // Named so a page can print "installed by N readers" without a second request, and documented as a census rather than a total.
        "install_count": install_count,
        "reviews": reviews,
    })))
}

async fn my_installations(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<Value>> {
    let installations = darkhold_db::gallery::my_installations(state.db(), &acting_scope(&user)?)
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?;
    Ok(Json(json!({ "installations": installations })))
}

/// The licence check a fork must pass before it is created.
///
/// Exposed as a function rather than inlined in the fork route because the fork route already copies the document byte-for-byte — the
/// licence it will carry is the source's — and the question "may this be forked at all" is a gallery rule, not a marketplace one.
pub fn fork_licence_allows(source_license: &str, derived_license: &str) -> Result<(), GalleryRefusal> {
    compatible(source_license, derived_license)
}
