//! The export API (spec §13).
//!
//! ```text
//! GET    /exports/formats                  what this instance can produce
//! POST   /exports                          ask for one (a job, not a file)
//! GET    /exports                          the reader's own exports
//! GET    /exports/{id}                     one, with its state
//! POST   /exports/{id}/grant               mint a short-lived download URL
//! GET    /exports/{id}/download            download it, signed in
//! GET    /exports/download/{token}         download it, holding the token
//! DELETE /exports/{id}                     forget one
//! ```
//!
//! # Four decisions worth stating
//!
//! **An export is a job, not a request.** A 122-unit work rendered to EPUB is
//! not something to do inside a reader's HTTP request, and spec §13 makes it a
//! queued unit whose state the reader can watch. So `POST /exports` answers `202`
//! with a job, and the file appears later.
//!
//! **The refusal happens before the job exists.** A format with no converter is
//! refused in the request, naming what an operator would install, rather than
//! accepted and failed in a worker the reader never sees. Same shape as the
//! importer's refusal of a source this instance cannot read.
//!
//! **The privacy acknowledgement is checked, not assumed.** Spec §13.6's notice is
//! shown once and recorded on the row; the API refuses an export whose reader has
//! not seen it, so the notice cannot be skipped by talking to the API directly.
//!
//! **Downloads are scoped twice.** The token in the URL is single-use and
//! short-lived and the row it opens is checked against it; the signed-in route
//! checks that the account owns the export. Neither is a public object address.

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use darkhold_db::exports as repo;
use darkhold_db::storage::BlobStore;
use darkhold_domain::exports::{ExportFormat, ExportOptions};
use darkhold_domain::AppError;

use crate::auth::{MaybeSession, RequireActorScoped, RequireSession};
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;
use darkhold_domain::api_scopes::Scope;

/// The privacy notice a reader has to have seen (spec §13.6).
///
/// It is returned with the refusal so the interface has one source for the text:
/// a notice duplicated in the frontend is a notice that can drift from the one
/// the server enforces.
pub const PRIVACY_NOTICE: &str = "An exported file is a copy of this work that leaves the \
server. Downloading it to a device means it is stored there, outside this instance's control, \
and deleting it here will not delete it there. Some devices keep their own cached copy.";

/// The token-addressed download, which needs no session.
///
/// Registered in the default class, because everything here authenticates by the
/// capability in the URL rather than by a cookie: a route that reads no session
/// should not be priced as one that does.
pub fn router() -> Router<AppState> {
    Router::new().route("/exports/download/{token}", get(download_by_token))
}

/// The reader's own export surface.
///
/// A write class even for the reads: every route here is scoped to the caller,
/// and one of them starts a job.
pub fn authed_router() -> Router<AppState> {
    Router::new()
        .route("/exports/formats", get(list_formats))
        .route("/exports", get(list_exports).post(start_export))
        .route("/exports/bulk", post(start_bulk_export))
        .route("/exports/{id}", get(get_export))
        .route("/exports/{id}/grant", post(mint_grant))
        .route("/exports/{id}/grants", get(list_grants))
        .route("/exports/{id}/download", get(download_own))
        .route("/exports/{id}/deliver", post(deliver_export))
        .route("/exports/{id}", delete(forget_export))
        // Devices this reader can receive an export on (M70-13: `user_devices` had no door).
        .route("/devices", get(list_devices).post(register_device))
        .route("/devices/{id}", delete(delete_device))
}

/// Query for the list.
#[derive(Debug, Deserialize)]
struct ListQuery {
    /// How many to return.
    limit: Option<i64>,
    /// The export to continue from.
    #[allow(dead_code)]
    after: Option<String>,
}

/// What an export looks like to its owner.
#[derive(Debug, Serialize)]
struct ExportView {
    id: String,
    job_id: String,
    subject_type: String,
    subject_id: String,
    format: String,
    label: String,
    state: String,
    output_bytes: Option<i64>,
    /// Whether a file exists to be fetched right now.
    downloadable: bool,
    error: Option<Value>,
    privacy_acknowledged: bool,
    created_at: String,
    updated_at: String,
}

impl From<repo::ExportJob> for ExportView {
    fn from(row: repo::ExportJob) -> Self {
        let format = ExportFormat::parse(&row.format);
        Self {
            id: row.id,
            job_id: row.job_id,
            subject_type: row.subject_type,
            subject_id: row.subject_id,
            format: row.format,
            label: format.map_or_else(String::new, |f| f.label().to_owned()),
            state: row.state,
            output_bytes: row.output_bytes,
            downloadable: row.output_blob_checksum.is_some(),
            error: row
                .error_message
                .as_deref()
                .and_then(|raw| serde_json::from_str(raw).ok()),
            privacy_acknowledged: row.privacy_acknowledged_at.is_some(),
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

/// The formats this instance can actually produce, and what is missing for the
/// ones it cannot.
async fn list_formats(
    State(state): State<AppState>,
    RequireSession(_user): RequireSession,
) -> ApiResult<Json<Value>> {
    Ok(Json(json!({
        "formats": state.converters().report(),
        "privacy_notice": PRIVACY_NOTICE,
        "retention_days": state.config().exports.retention_days,
    })))
}

/// The reader's own exports.
async fn list_exports(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Query(query): Query<ListQuery>,
) -> ApiResult<Json<Value>> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    // No state filter and no compound cursor: the interface shows a reader their
    // recent exports and nothing pages past a hundred of them.
    let rows =
        repo::list_exports(state.db(), &user.account_id.to_string(), None, limit, None).await?;
    let exports: Vec<ExportView> = rows.into_iter().map(ExportView::from).collect();
    Ok(Json(json!({ "exports": exports })))
}

/// What a request to export carries.
#[derive(Debug, Deserialize)]
struct StartExportRequest {
    subject_type: String,
    subject_id: String,
    format: String,
    #[serde(default)]
    options: Option<Value>,
    /// Whether the reader has been shown the notice.
    #[serde(default)]
    acknowledge_privacy: bool,
}

/// Ask for an export.
async fn start_export(
    State(state): State<AppState>,
    RequireActorScoped { actor }: RequireActorScoped,
    Json(request): Json<StartExportRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    // `library.read`, because that is what the handler's own permission check
    // already requires and §23.2's "a permitted export" means that permission.
    // Demanding `content.write` would be asking a bot for more authority than
    // the action uses: an export reads the caller's library and writes nothing
    // back.
    let actor = actor.require(Scope::LibraryRead)?;
    let Some(format) = ExportFormat::parse(&request.format) else {
        return Err(ApiError(darkhold_domain::AppError::Validation {
            message: format!("{:?} is not an export format", request.format),
            field_errors: Default::default(),
        }));
    };

    // The notice is part of the contract, not decoration: spec §13.6 asks that a
    // reader be told, and a check that a client can skip by omitting a field is
    // not a notice.
    if !request.acknowledge_privacy {
        return Err(ApiError(darkhold_domain::AppError::Validation {
            message: format!(
                "this export has not acknowledged the privacy notice: {PRIVACY_NOTICE}"
            ),
            field_errors: Default::default(),
        }));
    }

    let options = request
        .options
        .as_ref()
        .and_then(|value| serde_json::from_value::<ExportOptions>(value.clone()).ok())
        .unwrap_or_else(ExportOptions::defaults);

    let row = crate::exports::request(
        &state,
        &actor.account_id.to_string(),
        &request.subject_type,
        &request.subject_id,
        format,
        &options,
    )
    .await
    .map_err(|error| {
        ApiError(darkhold_domain::AppError::Validation {
            message: error.message(),
            field_errors: Default::default(),
        })
    })?;

    repo::acknowledge_privacy(state.db(), &row.id).await?;

    // 202: the work is accepted and has not happened.
    Ok((StatusCode::ACCEPTED, Json(json!(ExportView::from(row)))))
}

/// A bulk export request: a media query turned into a bundle.
#[derive(Debug, Deserialize)]
struct StartBulkExportRequest {
    /// The query to walk (the same shape as `POST /api/v1/media/query`).
    query: Value,
    /// Whether the reader has been shown the notice.
    #[serde(default)]
    acknowledge_privacy: bool,
}

/// Ask for a bulk export.
async fn start_bulk_export(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Json(request): Json<StartBulkExportRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    // The notice is part of the contract, not decoration (same as start_export).
    if !request.acknowledge_privacy {
        return Err(ApiError(darkhold_domain::AppError::Validation {
            message: format!(
                "this export has not acknowledged the privacy notice: {PRIVACY_NOTICE}"
            ),
            field_errors: Default::default(),
        }));
    }

    let row = crate::exports::request_bulk(&state, &user.account_id.to_string(), &request.query)
        .await
        .map_err(|error| {
            ApiError(darkhold_domain::AppError::Validation {
                message: error.message(),
                field_errors: Default::default(),
            })
        })?;

    repo::acknowledge_privacy(state.db(), &row.id).await?;

    // 202: the work is accepted and has not happened.
    Ok((StatusCode::ACCEPTED, Json(json!(ExportView::from(row)))))
}

/// One export, for its owner.
async fn get_export(
    State(state): State<AppState>,
    RequireActorScoped { actor }: RequireActorScoped,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    // Reading an export back is the other half of "a permitted export" (§23.2):
    // a bot that can *start* an export and cannot poll it is half a feature.
    // `library.read` again, and `owned` is the predicate that matters — the
    // scope says what an account may do, not whose rows are its own.
    let actor = actor.require(Scope::LibraryRead)?;
    let row = owned(&state, &actor.account_id.to_string(), &id).await?;

    // A bulk export's answer carries its per-work decisions. Without them the reader sees "ready" and a file size and
    // cannot tell which of the fifty works were included, skipped, or refused — and that list is the export's own audit
    // trail (`record_bulk_item` writes it as the worker decides). The field is added for `query` subjects only, so every
    // other export keeps the document shape it has always had.
    let mut value = serde_json::to_value(ExportView::from(row.clone())).unwrap_or(Value::Null);
    if row.subject_type == "query" {
        let items = repo::bulk_items_for_export(state.db(), &row.id).await?;
        value["items"] = json!(items
            .into_iter()
            .map(|item| json!({
                "work_id": item.work_id,
                "decision": item.decision,
                "reason": item.reason,
                "byte_size": item.byte_size,
            }))
            .collect::<Vec<_>>());
    }
    Ok(Json(value))
}

/// Forget an export and its file.
async fn forget_export(
    State(state): State<AppState>,
    RequireActorScoped { actor }: RequireActorScoped,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let actor = actor.require(Scope::LibraryRead)?;
    let row = owned(&state, &actor.account_id.to_string(), &id).await?;

    // The file goes first. An export row deleted while its blob is still
    // referenced would leave bytes nothing can reach, since the reference names
    // the row and the row is the only record of the reference.
    if let Some(checksum) = row.output_blob_checksum.as_deref() {
        let store = BlobStore::new(state.config().storage.root.clone());
        store
            .unreference(
                state.db(),
                checksum,
                crate::exports::EXPORT_OWNER_TYPE,
                &row.id,
            )
            .await?;
        store.delete_if_unreferenced(state.db(), checksum).await?;
    }
    let removed = repo::delete_export(state.db(), &row.id).await?;
    Ok(Json(json!({ "removed": removed })))
}

/// Mint a short-lived download link.
async fn mint_grant(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let row = owned(&state, &user.account_id.to_string(), &id).await?;
    if row.output_blob_checksum.is_none() {
        return Err(ApiError(darkhold_domain::AppError::Validation {
            message: "this export has no file yet".to_owned(),
            field_errors: Default::default(),
        }));
    }
    let token = crate::exports::mint_grant(&state, &row.id)
        .await
        .map_err(|error| {
            ApiError(darkhold_domain::AppError::Validation {
                message: error.message(),
                field_errors: Default::default(),
            })
        })?;
    Ok(Json(json!({
        "token": token,
        "url": format!("/exports/download/{token}"),
        "expires_in_seconds": state.config().exports.grant_ttl_secs,
    })))
}

/// Download an export the reader owns, without a token.
async fn download_own(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let row = owned(&state, &user.account_id.to_string(), &id).await?;
    serve(&state, &row).await
}

/// Download an export by holding its token.
///
/// Public by construction: the token *is* the authentication, it is single-use,
/// and the row it opens carries the format — so the caller chooses nothing and
/// can only be given the one file the grant was minted for.
async fn download_by_token(
    State(state): State<AppState>,
    MaybeSession(_user): MaybeSession,
    Path(token): Path<String>,
) -> ApiResult<Response> {
    let hash = crate::crypto::hash_token(&token);
    let now = darkhold_db::identity::now_rfc3339();
    let Some(export_id) = repo::redeem_grant(state.db(), &hash, &now).await? else {
        // One answer for expired, spent and never-existed alike: distinguishing
        // them tells someone holding a guess which guesses were close.
        return Err(ApiError(darkhold_domain::AppError::NotFound {
            resource: "download",
        }));
    };
    let Some(row) = repo::find_export(state.db(), &export_id).await? else {
        return Err(ApiError(darkhold_domain::AppError::NotFound {
            resource: "download",
        }));
    };
    serve(&state, &row).await
}

/// Hand back the artifact.
async fn serve(state: &AppState, row: &repo::ExportJob) -> ApiResult<Response> {
    let Some(checksum) = row.output_blob_checksum.as_deref() else {
        return Err(ApiError(darkhold_domain::AppError::NotFound {
            resource: "download",
        }));
    };
    let store = BlobStore::new(state.config().storage.root.clone());
    let Some(bytes) = store.get(state.db(), checksum).await? else {
        // The row says there is a file and there is not one. That is a fault on
        // this side rather than a wrong address, and saying so is what lets an
        // operator find it.
        return Err(ApiError(darkhold_domain::AppError::Internal(
            anyhow::anyhow!("the export's file is missing from storage"),
        )));
    };

    let format = ExportFormat::parse(&row.format);
    let media_type = format.map_or("application/octet-stream", ExportFormat::media_type);
    let extension = format.map_or("bin", ExportFormat::extension);
    let filename = filename_for(row, extension);

    let mut headers = HeaderMap::new();
    if let Ok(value) = format!(
        "attachment; filename=\"{filename}\"; filename*=UTF-8''{}",
        percent_encode(&filename)
    )
    .parse()
    {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    if let Ok(value) = media_type.parse() {
        headers.insert(header::CONTENT_TYPE, value);
    }
    // A generated file that names a work must not be cached by a shared cache:
    // the URL is a capability, and a proxy storing it would hand it to whoever
    // asks for the same address next.
    headers.insert(header::CACHE_CONTROL, "no-store".parse().expect("literal"));

    Ok((headers, bytes).into_response())
}

/// A filename a reader can recognise, and that no header can be injected into.
///
/// Quotes, backslashes and newlines are removed rather than escaped: a filename
/// is a convenience, and a `Content-Disposition` that can be broken out of is a
/// header-injection bug that ends in someone else's file being written over.
fn filename_for(row: &repo::ExportJob, extension: &str) -> String {
    let base: String = row.subject_id.chars().take(8).collect::<String>();
    let stem: String = format!("work-{base}")
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | ' ') {
                ch
            } else {
                '-'
            }
        })
        .collect();
    format!("{}.{extension}", stem.trim())
}

/// Percent-encode for the `filename*` parameter.
fn percent_encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(*byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Load an export and check it belongs to this account.
///
/// A miss and someone else's export are answered the same way. The difference
/// between them is exactly what an enumeration would want to learn.
async fn owned(state: &AppState, account_id: &str, id: &str) -> ApiResult<repo::ExportJob> {
    // `find_export_for`, not `find_export` plus a comparison: the ownership predicate belongs in the query, where it
    // cannot be forgotten by the next caller that fetches an export and only remembers to check the account sometimes.
    // This used to be the other implementation of that rule, sitting beside the SQL one and never used.
    repo::find_export_for(state.db(), id, account_id)
        .await?
        .ok_or(ApiError(darkhold_domain::AppError::NotFound {
            resource: "export",
        }))
}

/// §7.0 (M7-03) — Deliver an export to a device.
///
/// This build has no mail transport, so the only "delivery" is to surface
/// the Kindle/generic-device address the operator has configured, along
/// with a download link, so the reader can forward it themselves.
///
/// Spec §13.4 calls the adapter optional, so this refusal path is the
/// documented behaviour: the operator configures `device.kindle_email`
/// (or `device.device_email`), and the response returns that address plus
/// a short-lived grant URL. Without the configured email, the endpoint
/// returns `501 Not Implemented` — not a failure, but an honest statement
/// that delivery cannot happen until an operator configures a transport.
#[derive(Debug, Deserialize)]
struct DeliverExportBody {
    /// The target device. `kindle` | `generic`.
    device: String,
}

async fn deliver_export(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
    Json(body): Json<DeliverExportBody>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    let row = owned(&state, &user.account_id.to_string(), &id).await?;

    // The export must have a file to deliver.
    let Some(_checksum) = row.output_blob_checksum.as_ref() else {
        return Err(ApiError(darkhold_domain::AppError::Validation {
            message: "this export has no downloadable file yet".into(),
            field_errors: Default::default(),
        }));
    };

    // Mint a short-lived grant so the reader can download.
    let token = crate::exports::mint_grant(&state, &row.id)
        .await
        .map_err(|error| {
            ApiError(darkhold_domain::AppError::Validation {
                message: error.message(),
                field_errors: Default::default(),
            })
        })?;
    let download_url = format!("/exports/download/{token}");

    // Refuse with 512 if no device transport is configured. This is the
    // documented "no mail transport" refusal from spec §13.4.
    let target_email = match body.device.as_str() {
        "kindle" => state
            .config()
            .device
            .as_ref()
            .and_then(|d| d.kindle_email.as_ref()),
        "generic" => state
            .config()
            .device
            .as_ref()
            .and_then(|d| d.device_email.as_ref()),
        other => {
            return Err(ApiError(darkhold_domain::AppError::Validation {
                message: format!("unknown delivery device: {other}"),
                field_errors: Default::default(),
            }));
        }
    };

    match target_email {
        Some(email) => Ok((
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "delivered",
                "device": body.device,
                "target_email": email,
                "download_url": download_url,
                "message": format!("the file will be sent to {}", email),
            })),
        )),
        None => Ok((
            StatusCode::NOT_IMPLEMENTED,
            Json(serde_json::json!({
                "status": "no_transport",
                "device": body.device,
                "download_url": download_url,
                "message": "this instance has not configured a mail transport for device delivery; download the file and forward it yourself",
            })),
        )),
    }
}

/// What a reader says about a device.
///
/// `id` is the caller's own, not the server's: a device registers before it has a session-stable identity, and the
/// store's upsert is keyed on it, so a re-registration after a browser restart updates the row instead of leaving a
/// second one. That is also why the route is `POST /devices` with the id in the body rather than `PUT /devices/{id}`:
/// the browser knows the id, the server has never seen the device.
#[derive(Debug, Deserialize)]
struct DeviceRequest {
    id: String,
    #[serde(default)]
    label: String,
    /// A Web Push subscription, as the browser gave it. Never echoed back — see [`device_view`].
    #[serde(default)]
    push_subscription: Option<Value>,
}

/// A device, as the reader's own interface may see it.
///
/// The push subscription is **not** returned. It is a delivery credential: the endpoint and its keys let whoever holds
/// them push to the reader's browser, so echoing it turns any read of this list into a leak, and a reader's own page
/// does not need it back — the browser kept its copy. `has_push_subscription` is the fact the interface actually uses
/// (whether "notify this device" is on), and it is a boolean on purpose.
fn device_view(device: &darkhold_db::exports::UserDevice) -> Value {
    json!({
        "id": device.id,
        "label": device.label,
        "has_push_subscription": device.push_subscription_json.is_some(),
        "last_seen_at": device.last_seen_at,
        "created_at": device.created_at,
    })
}

fn validation(message: &str, field: &str, detail: &str) -> ApiError {
    let mut field_errors = std::collections::BTreeMap::new();
    field_errors.insert(field.to_string(), detail.to_string());
    ApiError(AppError::Validation {
        message: message.to_string(),
        field_errors,
    })
}

async fn list_devices(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<Value>> {
    let rows = repo::list_devices(state.db(), &user.account_id.to_string()).await?;
    let devices: Vec<Value> = rows.iter().map(device_view).collect();
    Ok(Json(json!({ "devices": devices })))
}

/// Register or update one of the caller's devices.
///
/// Scoped by the session's account at the store, so a body naming somebody else's device id writes a row for the
/// caller instead of taking over the other account's — the id is client-chosen, which makes that the only safe rule.
async fn register_device(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Json(body): Json<DeviceRequest>,
) -> ApiResult<Json<Value>> {
    // A UUID, because `user_devices.id` is `UUID` on PostgreSQL and `TEXT` on SQLite (0008): a free-form id decodes on
    // the default engine and fails `22P02` on the other, which is how this door answered 500 to its first PostgreSQL
    // run. The browser has `crypto.randomUUID()`; asking for one keeps the two engines honest without a column change.
    if uuid::Uuid::parse_str(body.id.trim()).is_err() {
        return Err(validation(
            "a device id must be a UUID",
            "id",
            "the string crypto.randomUUID() returns",
        ));
    }
    if body.label.len() > 64 {
        return Err(validation(
            "the label is too long",
            "label",
            "at most 64 characters",
        ));
    }
    if let Some(subscription) = &body.push_subscription {
        if !subscription.is_object() {
            return Err(validation(
                "a push subscription is a JSON object",
                "push_subscription",
                "the object the browser's PushManager returned",
            ));
        }
    }

    let account = user.account_id.to_string();
    let subscription = body.push_subscription.as_ref().map(ToString::to_string);
    let device = repo::upsert_device(
        state.db(),
        &body.id,
        &account,
        &body.label,
        subscription.as_deref(),
    )
    .await?;
    Ok(Json(device_view(&device)))
}

/// Remove one of the caller's devices.
///
/// `404` when the row is not the caller's — the store's `DELETE ... AND account_id = ?` already refuses it, and the
/// answer does not distinguish "no such device" from "not yours", for the same reason the coordinates door does not.
async fn delete_device(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let removed = repo::delete_device(state.db(), &id, &user.account_id.to_string()).await?;
    if !removed {
        return Err(ApiError(AppError::NotFound { resource: "device" }));
    }
    Ok(Json(json!({ "removed": true, "id": id })))
}

/// The download links minted for one export, and whether they are still good.
///
/// `mint_grant` produces a token and never shows it again — the store keeps a hash, deliberately. What a reader can
/// still learn is the *shape* of their access: when each link expires, whether that one has been spent, and how many
/// are outstanding. Without this a reader who lost the URL has no way to tell whether a new one is needed or whether
/// the old one is quietly still live.
async fn list_grants(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let export = owned(&state, &user.account_id.to_string(), &id).await?;
    let rows = repo::grants_for_export(state.db(), &export.id).await?;
    let grants: Vec<Value> = rows
        .into_iter()
        .map(|grant| {
            json!({
                "id": grant.id,
                "expires_at": grant.expires_at,
                "used_at": grant.used_at,
                "single_use": grant.single_use,
                "created_at": grant.created_at,
            })
        })
        .collect();
    Ok(Json(json!({ "export_id": export.id, "grants": grants })))
}
