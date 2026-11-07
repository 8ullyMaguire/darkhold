//! M16 — Marketplace routes: listings, commissions, extensions, webhooks, gallery.

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use std::str::FromStr;
use uuid::Uuid;

use crate::auth::{MaybeSession, RequireSession};
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

/// List listings.
pub async fn list_listings(
    State(state): State<AppState>,
    MaybeSession(_user): MaybeSession,
) -> ApiResult<Json<Value>> {
    let items = darkhold_db::marketplace::list_listings(state.db(), None, None, 50)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;
    Ok(Json(json!({ "listings": items })))
}

/// Create a listing.
#[derive(Debug, Deserialize)]
pub struct CreateListingBody {
    pub kind: String,
    pub work_id: Option<String>,
    pub terms: Value,
}

pub async fn create_listing(
    State(state): State<AppState>,
    MaybeSession(user): MaybeSession,
    Json(body): Json<CreateListingBody>,
) -> ApiResult<Json<Value>> {
    let account = user.map(|u| u.account_id.to_string()).unwrap_or_default();
    if account.is_empty() {
        return Err(ApiError(darkhold_domain::AppError::field(
            "session",
            "sign in to create listings",
        )));
    }

    let kind = darkhold_domain::marketplace::ListingKind::from_str(&body.kind)
        .map_err(|e| ApiError(darkhold_domain::AppError::field("kind", &e)))?;

    let terms = serde_json::to_string(&body.terms).unwrap();
    let id = darkhold_db::marketplace::create_listing(
        state.db(),
        kind,
        &account,
        body.work_id.as_deref(),
        &terms,
    )
    .await
    .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;

    Ok(Json(json!({ "id": id })))
}

/// Create a commission.
#[derive(Debug, Deserialize)]
pub struct CreateCommissionBody {
    pub listing_id: String,
}

pub async fn create_commission(
    State(state): State<AppState>,
    MaybeSession(user): MaybeSession,
    Json(body): Json<CreateCommissionBody>,
) -> ApiResult<Json<Value>> {
    let account = user.map(|u| u.account_id.to_string()).unwrap_or_default();
    if account.is_empty() {
        return Err(ApiError(darkhold_domain::AppError::field(
            "session",
            "sign in to request commissions",
        )));
    }

    let id = darkhold_db::marketplace::create_commission(state.db(), &body.listing_id, &account)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;

    Ok(Json(json!({ "id": id })))
}

/// Transition a commission.
#[derive(Debug, Deserialize)]
pub struct TransitionCommissionBody {
    pub from_state: String,
    pub to_state: String,
    pub ledger_ref: Option<String>,
}

pub async fn transition_commission(
    State(state): State<AppState>,
    Path(commission_id): Path<String>,
    MaybeSession(_user): MaybeSession,
    Json(body): Json<TransitionCommissionBody>,
) -> ApiResult<Json<Value>> {
    let from = darkhold_domain::marketplace::CommissionState::from_str(&body.from_state)
        .map_err(|e| ApiError(darkhold_domain::AppError::field("from_state", &e)))?;
    let to = darkhold_domain::marketplace::CommissionState::from_str(&body.to_state)
        .map_err(|e| ApiError(darkhold_domain::AppError::field("to_state", &e)))?;

    darkhold_db::marketplace::transition_commission(
        state.db(),
        &commission_id,
        &from,
        &to,
        body.ledger_ref.as_deref(),
    )
    .await
    .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;

    Ok(Json(json!({ "success": true })))
}

/// List extensions submitted by the caller.
pub async fn list_extensions(
    State(state): State<AppState>,
    MaybeSession(user): MaybeSession,
) -> ApiResult<Json<Value>> {
    let account = user.map(|u| u.account_id.to_string()).unwrap_or_default();
    if account.is_empty() {
        return Err(ApiError(darkhold_domain::AppError::field(
            "session",
            "sign in to list extensions",
        )));
    }
    let extensions = darkhold_db::marketplace::list_extensions(state.db(), Some(&account))
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;
    Ok(Json(json!({ "extensions": extensions })))
}

/// Get a single extension by slug. Returns 404 for unknown slug.
pub async fn get_extension(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    MaybeSession(_user): MaybeSession,
) -> ApiResult<Json<Value>> {
    let ext = darkhold_db::marketplace::get_extension(state.db(), &slug)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;
    match ext {
        Some(e) => Ok(Json(json!({ "extension": e }))),
        None => Err(ApiError(darkhold_domain::AppError::NotFound {
            resource: "extension",
        })),
    }
}

/// Grant an extension.
#[derive(Debug, Deserialize)]
pub struct GrantExtensionBody {
    pub manifest_id: String,
    pub version: String,
    pub capabilities: Vec<String>,
}

pub async fn grant_extension(
    State(state): State<AppState>,
    MaybeSession(user): MaybeSession,
    Json(body): Json<GrantExtensionBody>,
) -> ApiResult<Json<Value>> {
    let account = user.map(|u| u.account_id.to_string()).unwrap_or_default();
    if account.is_empty() {
        return Err(ApiError(darkhold_domain::AppError::field(
            "session",
            "sign in to grant extensions",
        )));
    }

    let capabilities: Vec<darkhold_domain::extension::Capability> = body
        .capabilities
        .iter()
        .map(|c| darkhold_domain::extension::Capability::from_str(c))
        .collect::<Result<_, _>>()
        .map_err(|e| ApiError(darkhold_domain::AppError::field("capabilities", &e)))?;

    darkhold_db::marketplace::grant_extension(
        state.db(),
        &account,
        &body.manifest_id,
        &body.version,
        &capabilities,
    )
    .await
    .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;

    Ok(Json(json!({ "granted": true })))
}

/// Revoke an extension.
pub async fn revoke_extension(
    State(state): State<AppState>,
    Path(manifest_id): Path<String>,
    MaybeSession(user): MaybeSession,
) -> ApiResult<Json<Value>> {
    let account = user.map(|u| u.account_id.to_string()).unwrap_or_default();
    if account.is_empty() {
        return Err(ApiError(darkhold_domain::AppError::field(
            "session",
            "sign in to revoke extensions",
        )));
    }

    darkhold_db::marketplace::revoke_extension(state.db(), &account, &manifest_id)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;

    Ok(Json(json!({ "revoked": true })))
}

/// List my extension grants.
pub async fn list_my_grants(
    State(state): State<AppState>,
    MaybeSession(user): MaybeSession,
) -> ApiResult<Json<Value>> {
    let account = user.map(|u| u.account_id.to_string()).unwrap_or_default();
    if account.is_empty() {
        return Err(ApiError(darkhold_domain::AppError::field(
            "session",
            "sign in to list grants",
        )));
    }
    let grants = darkhold_db::marketplace::list_my_grants(state.db(), &account)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;
    Ok(Json(json!({ "grants": grants })))
}

/// List webhooks for the caller.
pub async fn list_webhooks(
    State(state): State<AppState>,
    MaybeSession(user): MaybeSession,
) -> ApiResult<Json<Value>> {
    let account = user.map(|u| u.account_id.to_string()).unwrap_or_default();
    if account.is_empty() {
        return Err(ApiError(darkhold_domain::AppError::field(
            "session",
            "sign in to list webhooks",
        )));
    }
    let webhooks = darkhold_db::marketplace::list_webhooks(state.db(), &account)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;
    Ok(Json(json!({ "webhooks": webhooks })))
}

/// Create a webhook.
#[derive(Debug, Deserialize)]
pub struct CreateWebhookBody {
    pub url: String,
    pub events: Vec<String>,
}

pub async fn create_webhook(
    State(state): State<AppState>,
    MaybeSession(user): MaybeSession,
    Json(body): Json<CreateWebhookBody>,
) -> ApiResult<Json<Value>> {
    let account = user.map(|u| u.account_id.to_string()).unwrap_or_default();
    if account.is_empty() {
        return Err(ApiError(darkhold_domain::AppError::field(
            "session",
            "sign in to create webhooks",
        )));
    }

    let secret = format!("whsec_{}", Uuid::new_v4().to_string().replace('-', ""));
    let id = darkhold_db::marketplace::create_webhook(
        state.db(),
        &account,
        &body.url,
        &secret,
        &body.events,
    )
    .await
    .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;

    Ok(Json(json!({ "id": id, "secret": secret })))
}

/// Delete a webhook endpoint.
#[derive(Debug, Deserialize)]
pub struct DeleteWebhookQuery {
    pub id: String,
}

pub async fn delete_webhook(
    State(state): State<AppState>,
    MaybeSession(user): MaybeSession,
    Query(query): Query<DeleteWebhookQuery>,
) -> ApiResult<Json<Value>> {
    let account = user.map(|u| u.account_id.to_string()).unwrap_or_default();
    if account.is_empty() {
        return Err(ApiError(darkhold_domain::AppError::field(
            "session",
            "sign in to delete webhooks",
        )));
    }

    darkhold_db::marketplace::delete_webhook(state.db(), &account, &query.id)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;

    Ok(Json(json!({ "deleted": query.id })))
}

/// List gallery items for a work.
pub async fn list_gallery(
    State(state): State<AppState>,
    Path(work_id): Path<String>,
    MaybeSession(_user): MaybeSession,
) -> ApiResult<Json<Value>> {
    let items = darkhold_db::marketplace::list_gallery_items(state.db(), &work_id)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;
    Ok(Json(json!({ "items": items })))
}

/// Add a gallery item.
#[derive(Debug, Deserialize)]
pub struct AddGalleryBody {
    pub media_type: String,
    pub storage_key: String,
    pub alt_text: String,
    pub sanitized_document: String,
}

pub async fn add_gallery_item(
    State(state): State<AppState>,
    Path(work_id): Path<String>,
    MaybeSession(user): MaybeSession,
    Json(body): Json<AddGalleryBody>,
) -> ApiResult<Json<Value>> {
    let account = user.map(|u| u.account_id.to_string()).unwrap_or_default();
    if account.is_empty() {
        return Err(ApiError(darkhold_domain::AppError::field(
            "session",
            "sign in to add gallery items",
        )));
    }

    let id = darkhold_db::marketplace::add_gallery_item(
        state.db(),
        &work_id,
        &account,
        &body.media_type,
        &body.storage_key,
        &body.alt_text,
        &body.sanitized_document,
    )
    .await
    .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;

    Ok(Json(json!({ "id": id })))
}

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/listings", get(list_listings).post(create_listing))
        .route("/listings/{id}/commissions", post(create_commission))
        .route("/commissions/{id}/transition", post(transition_commission))
        .route("/extensions", get(list_extensions))
        .route("/extensions/{slug}", get(get_extension))
        // M16-12: a fork is a new package derived from a published one. Before the grant routes, because a fork is what a reader does
        // first — and because a fork never inherits a grant, so the order matches the only legal sequence.
        .route("/extensions/{slug}/fork", post(fork_extension))
        .route("/extensions/{slug}/grant", post(grant_extension))
        .route("/extensions/{slug}/revoke", post(revoke_extension))
        .route("/me/extension-grants", get(list_my_grants))
        .route(
            "/me/webhooks",
            get(list_webhooks)
                .post(create_webhook)
                .delete(delete_webhook),
        )
        .route(
            "/works/{id}/gallery",
            get(list_gallery).post(add_gallery_item),
        )
}

/// The body of a fork: the new package's own id and version.
#[derive(Debug, serde::Deserialize)]
pub struct ForkBody {
    /// The slug the fork is published under. Different from the source's, which is why the route takes both.
    pub id: String,
    pub version: String,
    /// A licence to publish the fork under. Absent means "the source's", which is the compatible answer for every licence; present means
    /// the fork is relabelled, and §21.6 requires that relabelling to be checked rather than assumed.
    #[serde(default)]
    pub license: Option<String>,
}

/// Fork a published extension (`POST /extensions/{slug}/fork`).
///
/// A fork keeps attribution (`derived_from`) and **starts with no grants**: a reader consented to the original package's permissions, and
/// a copy of it is a different package. The refusal on a duplicate id is a `409` rather than a silent overwrite — two packages with one
/// name is how a marketplace loses track of which one was reviewed.
pub async fn fork_extension(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    // `RequireSession`, matching the `Authenticated` row in the route inventory: the audience table says the door refuses an anonymous
    // caller, and an extractor that admits one and then checks by hand is how a door and its row disagree.
    RequireSession(user): RequireSession,
    Json(body): Json<ForkBody>,
) -> ApiResult<Json<Value>> {
    let account = user.account_id.to_string();
    let id = body.id.trim().to_lowercase();
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_') {
        return Err(ApiError(darkhold_domain::AppError::field(
            "id",
            "a package id is lowercase letters, digits, `-` and `_`",
        )));
    }
    if id == slug {
        return Err(ApiError(darkhold_domain::AppError::field(
            "id",
            "a fork needs its own id; forking onto the original's name would overwrite it",
        )));
    }
    // A fork under a different licence is relabelled deliberately, and the check happens before anything is written: a copyleft source
    // may not be republished under a permissive licence, and an unrecognised one is refused rather than assumed compatible.
    let mut document_override: Option<String> = None;
    if let Some(requested) = body.license.as_deref().map(str::trim).filter(|text| !text.is_empty()) {
        let source_document = darkhold_db::marketplace::manifest_document_for_latest(state.db(), &slug)
            .await
            .map_err(|error| ApiError(darkhold_domain::AppError::Internal(error.into())))?
            .ok_or_else(|| ApiError(darkhold_domain::AppError::NotFound {
                resource: "extension",
            }))?;
        let mut manifest: darkhold_domain::extension::Manifest =
            serde_json::from_str(&source_document).map_err(|error| {
                ApiError(darkhold_domain::AppError::Internal(anyhow::Error::from(error)))
            })?;
        darkhold_domain::gallery::compatible(&manifest.license, requested).map_err(|refusal| {
            ApiError(darkhold_domain::AppError::Validation {
                message: refusal.message(),
                field_errors: Default::default(),
            })
        })?;
        if manifest.license != requested {
            manifest.license = requested.to_owned();
            document_override = Some(serde_json::to_string(&manifest).map_err(|error| {
                ApiError(darkhold_domain::AppError::Internal(anyhow::Error::from(error)))
            })?);
        }
    }

    match darkhold_db::marketplace::fork_manifest_document(
        state.db(),
        &slug,
        &id,
        body.version.trim(),
        &account,
        document_override.as_deref(),
    )
    .await
    {
        Ok(mut fork) => {
            // The attribution sentence a page can print, derived from the row rather than written by the caller. §21.6 says attribution is
            // preserved, which is a claim about what a reader sees, not only about a column. The licence comes from the fork's own
            // document: the row does not carry one, and the document is what the reader would be granting.
            if let (Some(source), Some(version)) = (
                fork.get("derived_from").and_then(serde_json::Value::as_str).map(str::to_owned),
                fork.get("derived_from_version").and_then(serde_json::Value::as_str).map(str::to_owned),
            ) {
                let license = darkhold_db::marketplace::manifest_document(state.db(), &id, body.version.trim())
                    .await
                    .ok()
                    .flatten()
                    .and_then(|document| {
                        serde_json::from_str::<darkhold_domain::extension::Manifest>(&document).ok()
                    })
                    .map(|manifest| manifest.license)
                    .unwrap_or_else(|| "licence unrecorded".to_owned());
                let sentence = darkhold_domain::gallery::attribution(&source, &version, &license);
                if let Some(object) = fork.as_object_mut() {
                    object.insert("attribution".to_owned(), json!(sentence));
                }
            }
            Ok(Json(fork))
        }
        Err(sqlx::Error::RowNotFound) => Err(ApiError(darkhold_domain::AppError::NotFound {
            resource: "extension",
        })),
        Err(sqlx::Error::Protocol(message)) => Err(ApiError(darkhold_domain::AppError::field("id", &message))),
        Err(error) => Err(ApiError(darkhold_domain::AppError::Internal(error.into()))),
    }
}
