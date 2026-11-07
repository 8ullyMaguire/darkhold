//! §21.5's review workflow over HTTP (M16-07).
//!
//! ```text
//! POST /api/v1/extensions/{slug}/submit                  the author's package enters review
//! GET  /api/v1/extensions/reviews?stage=permission_review the queue, oldest first
//! POST /api/v1/extensions/reviews/{slug}/{version}/checks  automated check results
//! POST /api/v1/extensions/reviews/{slug}/{version}/advance a human moves one stage on
//! POST /api/v1/extensions/reviews/{slug}/{version}/approve a quorum approver votes
//! POST /api/v1/extensions/reviews/{slug}/{version}/reject  refuse the submission
//! POST /api/v1/extensions/reviews/{slug}/{version}/revoke  emergency: pull it and roll back
//! ```
//!
//! # What these routes do not decide
//!
//! Nothing about who may say what. Every rule — no skipping, no self-review, two different reviewers, quorum, renewal on permission
//! growth — lives in `darkhold_domain::extension_review`, and a handler here calls it and stores the answer. That division is why the
//! rules have tests that do not need a database, and why a future route cannot add a fifth way to publish.
//!
//! # The operator door
//!
//! Every mutating route except `submit` is `Audience::Operator` and asks `require_operator` itself, so an anonymous call is a 401 and a
//! signed-in reader a 403. `submit` is `Authenticated`: an author submits their own package, and the state machine refuses them as a
//! reviewer afterwards — the one place in this module where "who is asking" changes which door it is.

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::RequireSession;
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;
use darkhold_domain::extension_review::{Refusal, Review, Stage};
use darkhold_domain::AppError;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/extensions/{slug}/submit", post(submit))
        .route("/extensions/reviews", get(queue_route))
        .route(
            "/extensions/reviews/{slug}/{version}/checks",
            post(record_checks),
        )
        .route(
            "/extensions/reviews/{slug}/{version}/advance",
            post(advance),
        )
        .route(
            "/extensions/reviews/{slug}/{version}/approve",
            post(approve),
        )
        .route("/extensions/reviews/{slug}/{version}/reject", post(reject))
        .route("/extensions/reviews/{slug}/{version}/revoke", post(revoke))
}

fn refused(refusal: Refusal) -> ApiError {
    // A refusal from the workflow is a rule, not a malformed request, so it is a `409` with the rule's own sentence: a reviewer needs to
    // know *which* rule stopped them, and `VALIDATION_FAILED` would send them looking at their JSON.
    ApiError(AppError::Validation {
        message: refusal.message(),
        field_errors: Default::default(),
    })
}

#[derive(Debug, Deserialize)]
pub struct SubmitBody {
    pub version: String,
    /// The manifest document, as the author wrote it.
    pub document: String,
}

/// Submit a package version for review (`POST /extensions/{slug}/submit`).
pub async fn submit(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(slug): Path<String>,
    Json(body): Json<SubmitBody>,
) -> ApiResult<Json<Value>> {
    let account = user.account_id.to_string();
    let version = body.version.trim().to_owned();
    if version.is_empty() {
        return Err(ApiError(AppError::field("version", "a submission needs a version")));
    }
    if body.document.trim().is_empty() {
        return Err(ApiError(AppError::field("document", "a submission needs a manifest")));
    }

    // The manifest row is created here, in `pending`: a submission is a package that exists and has not been approved, which is exactly
    // what `pending` means everywhere else in this table.
    if !darkhold_db::marketplace::submit_manifest(
        state.db(),
        &slug,
        &version,
        &body.document,
        &account,
    )
    .await
    .map_err(|error| match error {
        sqlx::Error::Protocol(message) => ApiError(AppError::field("version", &message)),
        other => ApiError(AppError::Internal(other.into())),
    })? {
        return Err(ApiError(AppError::field(
            "version",
            "this version of the package has already been submitted",
        )));
    }

    let review = Review::new(&account);
    darkhold_db::extension_review::save_review(state.db(), &slug, &version, &review, None)
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?;

    Ok(Json(json!({
        "manifest_id": slug,
        "version": version,
        "stage": review.stage.as_str(),
        "next": "record the automated checks",
    })))
}

#[derive(Debug, Deserialize)]
pub struct QueueQuery {
    #[serde(default)]
    pub stage: Option<String>,
}

async fn queue_route(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Query(query): Query<QueueQuery>,
) -> ApiResult<Json<Value>> {
    crate::routes::discovery::require_operator(&state, &user)?;
    let stage = match query.stage.as_deref() {
        // Default to the first human stage: the queue a reviewer opens is "what needs a person", and automated checks are not a person.
        None => Stage::PermissionReview,
        Some(name) => Stage::from_str(name).ok_or_else(|| {
            ApiError(AppError::field("stage", "that is not a review stage"))
        })?,
    };
    let items = darkhold_db::extension_review::queue(state.db(), stage, 100)
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?;
    Ok(Json(json!({ "stage": stage.as_str(), "items": items })))
}

#[derive(Debug, Deserialize)]
pub struct CheckBody {
    pub name: String,
    #[serde(default)]
    pub passed: bool,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChecksBody {
    pub checks: Vec<CheckBody>,
    /// The whole result set for this run of the automated checks. Replacing rather than appending because the checks are a *report*, and
    /// a package's report should say what the last run found, not what every run since submission found.
    #[serde(default)]
    pub replace: bool,
}

async fn record_checks(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path((slug, version)): Path<(String, String)>,
    Json(body): Json<ChecksBody>,
) -> ApiResult<Json<Value>> {
    crate::routes::discovery::require_operator(&state, &user)?;
    let mut review = load(&state, &slug, &version).await?;
    if body.replace || review.checks.is_empty() {
        review.checks.clear();
    }
    for check in &body.checks {
        review.checks.push(darkhold_domain::extension_review::Check {
            name: check.name.clone(),
            outcome: if check.passed {
                Ok(())
            } else {
                Err(check
                    .reason
                    .clone()
                    .unwrap_or_else(|| "failed, with no reason recorded".to_owned()))
            },
        });
    }
    let stage = review.apply(darkhold_domain::extension_review::Action::RecordChecks).map_err(refused)?;
    save(&state, &slug, &version, &review, None).await?;
    Ok(Json(json!({
        "stage": stage.as_str(),
        "checks_passed": review.checks_passed(),
        "checks": review.checks.iter().map(|check| json!({
            "name": check.name,
            "passed": check.outcome.is_ok(),
            "reason": check.outcome.as_ref().err(),
        })).collect::<Vec<_>>(),
    })))
}

async fn advance(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path((slug, version)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    crate::routes::discovery::require_operator(&state, &user)?;
    let mut review = load(&state, &slug, &version).await?;
    let stage = review
        .apply(darkhold_domain::extension_review::Action::Advance {
            actor: user.account_id.to_string(),
        })
        .map_err(refused)?;
    save(&state, &slug, &version, &review, None).await?;
    Ok(Json(json!({ "stage": stage.as_str() })))
}

async fn approve(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path((slug, version)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    crate::routes::discovery::require_operator(&state, &user)?;
    let mut review = load(&state, &slug, &version).await?;
    let actor = user.account_id.to_string();
    let outcome = review.apply(darkhold_domain::extension_review::Action::Approve {
        actor: actor.clone(),
    });
    // A duplicate or a self-review is an error worth returning; "not yet quorum" is not an error — the approval was accepted and the
    // submission simply is not published, and answering 4xx for it would make a correct action look like a mistake.
    match outcome {
        Ok(stage) => {
            save(&state, &slug, &version, &review, None).await?;
            darkhold_db::extension_review::set_manifest_state(state.db(), &slug, &version, "approved")
                .await
                .map_err(|error| ApiError(AppError::Internal(error.into())))?;
            // §21.5's renewal clause: publishing a version that asks for more than an existing grant covers invalidates that grant, and
            // the response says how many readers have to be asked again. Done at publish because that is the moment the wider
            // permission set becomes the thing on offer -- a check at grant time would ask the reader to notice the difference.
            let requested = declared_capabilities(&state, &slug, &version).await?;
            let renewed =
                darkhold_db::extension_review::renewal_on_growth(state.db(), &slug, &version, &requested)
                    .await
                    .map_err(|error| ApiError(AppError::Internal(error.into())))?;
            Ok(Json(json!({
                "stage": stage.as_str(),
                "approvals": review.approvals,
                "published": true,
                "renewed_consent": renewed,
            })))
        }
        Err(Refusal::QuorumNotReached { have, need }) => {
            save(&state, &slug, &version, &review, None).await?;
            Ok(Json(json!({
                "stage": review.stage.as_str(),
                "approvals": review.approvals,
                "published": false,
                "quorum": { "have": have, "need": need },
            })))
        }
        Err(refusal) => Err(refused(refusal)),
    }
}

#[derive(Debug, Deserialize)]
pub struct ReasonBody {
    #[serde(default)]
    pub reason: String,
}

async fn reject(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path((slug, version)): Path<(String, String)>,
    Json(body): Json<ReasonBody>,
) -> ApiResult<Json<Value>> {
    crate::routes::discovery::require_operator(&state, &user)?;
    let mut review = load(&state, &slug, &version).await?;
    let stage = review
        .apply(darkhold_domain::extension_review::Action::Reject {
            actor: user.account_id.to_string(),
            reason: body.reason.clone(),
        })
        .map_err(refused)?;
    save(&state, &slug, &version, &review, Some(&body.reason)).await?;
    darkhold_db::extension_review::set_manifest_state(state.db(), &slug, &version, "rejected")
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?;
    Ok(Json(json!({ "stage": stage.as_str(), "reason": body.reason })))
}

async fn revoke(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path((slug, version)): Path<(String, String)>,
    Json(body): Json<ReasonBody>,
) -> ApiResult<Json<Value>> {
    crate::routes::discovery::require_operator(&state, &user)?;
    let mut review = load(&state, &slug, &version).await?;
    let stage = review
        .apply(darkhold_domain::extension_review::Action::Revoke {
            actor: user.account_id.to_string(),
            reason: body.reason.clone(),
        })
        .map_err(refused)?;
    save(&state, &slug, &version, &review, Some(&body.reason)).await?;

    // Revocation is three facts, and all three have to land or the state is a lie: the manifest stops being approved, the version's
    // grants stop counting, and the caller is told which version readers fall back to.
    darkhold_db::extension_review::set_manifest_state(state.db(), &slug, &version, "revoked")
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?;
    let revoked_grants = darkhold_db::extension_review::require_renewal(state.db(), &slug, &version)
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?;
    let rollback_to = darkhold_db::extension_review::previous_published(state.db(), &slug, &version)
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?;

    Ok(Json(json!({
        "stage": stage.as_str(),
        "reason": body.reason,
        "grants_revoked": revoked_grants,
        // `null` means there is nothing to fall back to and the package is simply gone: saying so is better than naming a version that
        // does not exist.
        "rollback_to": rollback_to,
    })))
}

/// The capability set a submitted manifest declares.
///
/// A document that does not parse is not a reason to refuse publication here — the automated checks own that judgement, and this function
/// is answering "which readers must we ask again". An unparsable document therefore declares nothing, and the publish proceeds with
/// `renewed_consent: 0` rather than an error the reviewer cannot act on.
async fn declared_capabilities(
    state: &AppState,
    slug: &str,
    version: &str,
) -> ApiResult<std::collections::BTreeSet<String>> {
    let document = darkhold_db::marketplace::manifest_document(state.db(), slug, version)
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?;
    let Some(document) = document else {
        return Ok(std::collections::BTreeSet::new());
    };
    Ok(serde_json::from_str::<darkhold_domain::extension::Manifest>(&document)
        .map(|manifest| {
            manifest
                .permissions
                .iter()
                .map(|capability| capability.as_str().to_owned())
                .collect()
        })
        .unwrap_or_default())
}

async fn load(state: &AppState, slug: &str, version: &str) -> ApiResult<Review> {
    darkhold_db::extension_review::review_for(state.db(), slug, version)
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?
        .ok_or_else(|| {
            ApiError(AppError::NotFound {
                resource: "submission",
            })
        })
}

async fn save(
    state: &AppState,
    slug: &str,
    version: &str,
    review: &Review,
    reason: Option<&str>,
) -> ApiResult<()> {
    darkhold_db::extension_review::save_review(state.db(), slug, version, review, reason)
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))
}
