//! M70-09 — extension documents over HTTP: author, import, export, delete.
//!
//! ```text
//! POST   /api/v1/me/extension-documents            {document: {...}, "public": false}
//! GET    /api/v1/me/extension-documents            mine, newest first
//! GET    /api/v1/me/extension-documents/:id        one (mine, or somebody's public)
//! GET    /api/v1/me/extension-documents/:id/export canonical text, byte for byte
//! DELETE /api/v1/me/extension-documents/:id        mine only
//! ```
//!
//! **Authoring and importing are the same request.** There is no separate import endpoint, because a separate one
//! would be a second place to remember the validator — and the guarantee that a shared document cannot reach a
//! state the API refuses comes from there being exactly one path through `extension_document::parse`.
//!
//! **Export returns the stored text, not a re-serialisation.** The import already canonicalised it, so re-encoding
//! on the way out would be an opportunity to disagree with what was validated, for no benefit.

use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use darkhold_domain::error::AppError;
use darkhold_domain::extension_document::{parse, Refusal};

use crate::auth::{MaybeSession, RequireSession};
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

/// The documents routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/me/extension-documents", get(list).post(create))
        .route("/me/extension-documents/{id}", get(one).delete(remove))
        .route("/me/extension-documents/{id}/export", get(export))
}

#[derive(Debug, Deserialize)]
struct CreateBody {
    /// The document, as JSON. An object is preferred; a string is accepted because that is what an export is.
    document: Value,
    #[serde(default)]
    is_public: bool,
}

fn refused(refusal: &Refusal) -> ApiError {
    let message = refusal.to_string();
    let mut field_errors = std::collections::BTreeMap::new();
    field_errors.insert("document".to_owned(), message.clone());
    ApiError(AppError::Validation {
        message,
        field_errors,
    })
}

fn not_found() -> ApiError {
    // One answer for "not there", "not yours" and "private": a distinct refusal would confirm that a document id
    // exists, which is the beginning of an enumeration.
    ApiError(AppError::NotFound {
        resource: "document",
    })
}

async fn create(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Json(body): Json<CreateBody>,
) -> ApiResult<Json<Value>> {
    // A string is parsed as-is (this is the import path); an object is encoded first. Both end in the same validator.
    let raw = match &body.document {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    let document = parse(&raw).map_err(|refusal| refused(&refusal))?;
    let account = user.account_id.to_string();
    let id = darkhold_db::extension_documents::create(
        state.db(),
        &account,
        &document.to_json(),
        &document.name,
        document.kind.as_str(),
        body.is_public,
    )
    .await?;
    Ok(Json(json!({
        "id": id,
        "name": document.name,
        "kind": document.kind.as_str(),
        "is_public": body.is_public,
    })))
}

async fn list(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<Value>> {
    let account = user.account_id.to_string();
    let documents = darkhold_db::extension_documents::list_mine(state.db(), &account).await?;
    let items: Vec<Value> = documents
        .into_iter()
        .map(|document| {
            json!({
                "id": document.id,
                "name": document.name,
                "kind": document.kind,
                "is_public": document.is_public,
                "created_at": document.created_at,
                "export_url": format!("/api/v1/me/extension-documents/{}/export", document.id),
            })
        })
        .collect();
    Ok(Json(json!({ "items": items, "count": items.len() })))
}

async fn one(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let account = user.account_id.to_string();
    let Some(document) = darkhold_db::extension_documents::get(state.db(), &id, &account).await?
    else {
        return Err(not_found());
    };
    Ok(Json(json!({
        "id": document.id,
        "name": document.name,
        "kind": document.kind,
        "is_public": document.is_public,
        "created_at": document.created_at,
        "mine": document.account == account,
        "document": document.document,
    })))
}

async fn export(
    State(state): State<AppState>,
    MaybeSession(session): MaybeSession,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    // A public document is exportable without a session — that is what publishing it means — and a private one
    // needs its author, whose account the store compares for us.
    let viewer = session
        .map(|s| s.account_id.to_string())
        .unwrap_or_default();
    let Some(document) = darkhold_db::extension_documents::get(state.db(), &id, &viewer).await?
    else {
        return Err(not_found());
    };
    Ok((
        StatusCode::OK,
        [
            (
                header::CONTENT_TYPE,
                "application/json; charset=utf-8".to_owned(),
            ),
            (header::CACHE_CONTROL, "private, no-cache".to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}.json\"", document.kind),
            ),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_owned()),
        ],
        document.document,
    )
        .into_response())
}

async fn remove(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let account = user.account_id.to_string();
    let removed = darkhold_db::extension_documents::delete(state.db(), &id, &account).await?;
    if !removed {
        return Err(not_found());
    }
    Ok(Json(json!({ "deleted": true })))
}
