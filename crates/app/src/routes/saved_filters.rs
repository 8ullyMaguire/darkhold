//! M69-21 — a Saved Filter's public life: keybind, follow, blend.
//!
//! * `PATCH  /api/v1/saved-views/{id}/keybind`   — bind a filter to a digit (or clear it)
//! * `GET    /api/v1/me/keybinds`                — this reader's digits
//! * `POST   /api/v1/saved-views/{id}/follow`    — follow a public filter
//! * `DELETE /api/v1/saved-views/{id}/follow`
//! * `GET    /api/v1/me/followed-filters`
//! * `GET    /api/v1/saved-view-blends`          — this reader's blends
//! * `POST   /api/v1/saved-view-blends`          — {name, left, right, mode, pinned}
//! * `DELETE /api/v1/saved-view-blends/{id}`
//! * `GET    /api/v1/saved-view-blends/{id}/items` — the blend, answered
//!
//! # Why these are routes over an object that already exists
//!
//! `saved_views` has existed since 0009 and has had a slug since 0173. What none of it had was a reader's way to *use* one
//! quickly: a digit to press, a way to follow somebody else's, and a way to put two side by side. None of the three is a second
//! query engine — the blend runs the two filters the reader already has and interleaves the results — and none stores a filter
//! in a second shape.
//!
//! # The refusals are the design
//!
//! A keybind collision is refused rather than resolved (the reader chooses), a private filter cannot be followed (the owner's
//! next edit would publish into a stranger's feed), and a blend may only name the reader's own filters (naming somebody else's
//! private view would be a read of it through a new door).

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::RequireSession;
use crate::http::{ApiError, ApiResult};
use crate::routes::feeds::{catalogue_items, library_items};
use crate::state::AppState;
use darkhold_db::library::{self, SavedViewKind};
use darkhold_db::saved_filter_life;
use darkhold_domain::error::AppError;
use darkhold_domain::saved_filter::{blend, BlendMode};

/// The Saved Filter's public life.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/me/keybinds", get(list_keybinds))
        .route("/me/followed-filters", get(list_followed))
        .route(
            "/saved-views/{id}/keybind",
            axum::routing::patch(set_keybind),
        )
        .route("/saved-views/{id}/follow", post(follow).delete(unfollow))
        .route("/saved-view-blends", get(list_blends).post(create_blend))
        .route(
            "/saved-view-blends/{id}",
            axum::routing::delete(delete_blend),
        )
        .route("/saved-view-blends/{id}/items", get(blend_items))
}

fn closed(state: &AppState) -> Option<ApiError> {
    (!state.config().discovery.saved_filters).then(|| {
        ApiError(AppError::NotFound {
            resource: "saved filters",
        })
    })
}

fn internal(error: anyhow::Error) -> ApiError {
    ApiError(AppError::Internal(error))
}

fn parse<T: serde::de::DeserializeOwned>(body: &Bytes) -> Result<T, ApiError> {
    serde_json::from_slice(body).map_err(|error| {
        let mut field_errors = std::collections::BTreeMap::new();
        field_errors.insert("body".to_owned(), error.to_string());
        ApiError(AppError::Validation {
            message: "the request body is not valid".to_owned(),
            field_errors,
        })
    })
}

fn validation(message: String) -> ApiError {
    ApiError(AppError::Validation {
        message,
        field_errors: std::collections::BTreeMap::new(),
    })
}

#[derive(Debug, Deserialize)]
struct KeybindBody {
    /// `null` clears the binding; a digit 1–9 sets it.
    keybind: Option<i64>,
}

async fn set_keybind(
    State(state): State<AppState>,
    session: RequireSession,
    Path(id): Path<String>,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    if let Some(error) = closed(&state) {
        return Err(error);
    }
    let request: KeybindBody = parse(&body)?;
    let account = session.0.account_id.to_string();
    let now = darkhold_db::identity::now_rfc3339();
    let changed = saved_filter_life::set_keybind(state.db(), &account, &id, request.keybind, &now)
        .await
        .map_err(|error| validation(error.to_string()))?;
    if !changed {
        return Err(ApiError(AppError::NotFound {
            resource: "saved view",
        }));
    }
    Ok(Json(json!({
        "view_id": id,
        "keybind": request.keybind,
        "note": match request.keybind {
            Some(digit) => format!("press {digit} to open this filter"),
            None => "the filter is no longer bound to a key".to_owned(),
        },
    })))
}

async fn list_keybinds(
    State(state): State<AppState>,
    session: RequireSession,
) -> ApiResult<Json<Value>> {
    if let Some(error) = closed(&state) {
        return Err(error);
    }
    let account = session.0.account_id.to_string();
    let binds = saved_filter_life::keybinds(state.db(), &account)
        .await
        .map_err(internal)?;
    Ok(Json(json!({
        "keybinds": binds.iter().map(|(digit, view_id, name)| json!({
            "key": digit,
            "view_id": view_id,
            "name": name,
        })).collect::<Vec<Value>>(),
    })))
}

async fn follow(
    State(state): State<AppState>,
    session: RequireSession,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    if let Some(error) = closed(&state) {
        return Err(error);
    }
    let account = session.0.account_id.to_string();
    let now = darkhold_db::identity::now_rfc3339();
    let fresh = saved_filter_life::subscribe(state.db(), &account, &id, &now)
        .await
        .map_err(|error| validation(error.to_string()))?;
    let count = saved_filter_life::subscriber_count(state.db(), &id)
        .await
        .map_err(internal)?;
    Ok(Json(json!({
        "following": true,
        "already_followed": !fresh,
        "subscribers": count,
    })))
}

async fn unfollow(
    State(state): State<AppState>,
    session: RequireSession,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    if let Some(error) = closed(&state) {
        return Err(error);
    }
    let account = session.0.account_id.to_string();
    let changed = saved_filter_life::unsubscribe(state.db(), &account, &id)
        .await
        .map_err(internal)?;
    let count = saved_filter_life::subscriber_count(state.db(), &id)
        .await
        .map_err(internal)?;
    Ok(Json(json!({
        "following": false,
        "was_followed": changed,
        "subscribers": count,
    })))
}

async fn list_followed(
    State(state): State<AppState>,
    session: RequireSession,
) -> ApiResult<Json<Value>> {
    if let Some(error) = closed(&state) {
        return Err(error);
    }
    let account = session.0.account_id.to_string();
    let followed = saved_filter_life::followed(state.db(), &account)
        .await
        .map_err(internal)?;
    let base = state.config().site.base_url.trim_end_matches('/');
    Ok(Json(json!({
        "followed": followed.iter().map(|view| json!({
            "view_id": view.view_id,
            "name": view.name,
            "slug": view.slug,
            "url": view.slug.as_ref().map(|slug| format!("{base}/s/{slug}")),
            "followed_since": view.since,
            "owner": view.owner_account_id,
        })).collect::<Vec<Value>>(),
    })))
}

#[derive(Debug, Deserialize)]
struct BlendBody {
    name: String,
    left: String,
    right: String,
    /// `interleave` (default) or `union`.
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    pinned: Option<bool>,
}

async fn create_blend_route_body(
    state: &AppState,
    account: &str,
    request: BlendBody,
) -> ApiResult<Json<Value>> {
    let now = darkhold_db::identity::now_rfc3339();
    let mode = request.mode.as_deref().unwrap_or("interleave");
    let created = saved_filter_life::create_blend(
        state.db(),
        account,
        saved_filter_life::NewBlend {
            name: &request.name,
            left_view_id: &request.left,
            right_view_id: &request.right,
            mode,
            pinned: request.pinned.unwrap_or(false),
        },
        &now,
    )
    .await
    .map_err(|error| validation(error.to_string()))?;
    Ok(Json(json!({
        "blend": {
            "id": created.id,
            "name": created.name,
            "left": created.left_view_id,
            "right": created.right_view_id,
            "mode": created.mode,
            "pinned": created.pinned,
        },
    })))
}

async fn create_blend(
    State(state): State<AppState>,
    session: RequireSession,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    if let Some(error) = closed(&state) {
        return Err(error);
    }
    let request: BlendBody = parse(&body)?;
    let account = session.0.account_id.to_string();
    create_blend_route_body(&state, &account, request).await
}

async fn list_blends(
    State(state): State<AppState>,
    session: RequireSession,
) -> ApiResult<Json<Value>> {
    if let Some(error) = closed(&state) {
        return Err(error);
    }
    let account = session.0.account_id.to_string();
    let blends = saved_filter_life::blends(state.db(), &account)
        .await
        .map_err(internal)?;
    Ok(Json(json!({
        "blends": blends.iter().map(|row| json!({
            "id": row.id,
            "name": row.name,
            "left": row.left_view_id,
            "right": row.right_view_id,
            "mode": row.mode,
            "pinned": row.pinned,
        })).collect::<Vec<Value>>(),
    })))
}

async fn delete_blend(
    State(state): State<AppState>,
    session: RequireSession,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    if let Some(error) = closed(&state) {
        return Err(error);
    }
    let account = session.0.account_id.to_string();
    let deleted = saved_filter_life::delete_blend(state.db(), &account, &id)
        .await
        .map_err(internal)?;
    if !deleted {
        return Err(ApiError(AppError::NotFound { resource: "blend" }));
    }
    Ok(Json(json!({ "deleted": true })))
}

/// One side's rows, as `(id, title)` pairs, through the same path the feed uses.
async fn side_items(
    state: &AppState,
    account: &str,
    view_id: &str,
    limit: i64,
) -> ApiResult<Vec<(String, String)>> {
    let view = library::find_saved_view(state.db(), account, view_id)
        .await
        .map_err(internal)?
        .ok_or_else(|| {
            ApiError(AppError::NotFound {
                resource: "saved view",
            })
        })?;
    match view.kind {
        SavedViewKind::Catalogue => {
            // `kind` came back public on the struct; the enum itself is not `Clone`-copyable, so it is matched here.
            let entries = catalogue_items(state, &view, limit).await?;
            Ok(entries
                .into_iter()
                .map(|entry| (entry.id, entry.title))
                .collect())
        }
        SavedViewKind::Library => {
            let entries = library_items(state, account, &view, limit).await?;
            Ok(entries
                .into_iter()
                .map(|entry| (entry.id, entry.title))
                .collect())
        }
    }
}

async fn blend_items(
    State(state): State<AppState>,
    session: RequireSession,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    if let Some(error) = closed(&state) {
        return Err(error);
    }
    let account = session.0.account_id.to_string();
    let blend_row = saved_filter_life::find_blend(state.db(), &account, &id)
        .await
        .map_err(internal)?
        .ok_or_else(|| ApiError(AppError::NotFound { resource: "blend" }))?;
    let mode = BlendMode::parse(&blend_row.mode).map_err(validation)?;

    // Each side runs with the same bound, so a blend cannot be dominated by one filter's page size.
    let limit = 20;
    let left = side_items(&state, &account, &blend_row.left_view_id, limit).await?;
    let right = side_items(&state, &account, &blend_row.right_view_id, limit).await?;
    let blended = blend(&left, &right, mode);

    Ok(Json(json!({
        "blend": {
            "id": blend_row.id,
            "name": blend_row.name,
            "mode": blend_row.mode,
            "left": blend_row.left_view_id,
            "right": blend_row.right_view_id,
        },
        "items": blended.iter().map(|(id, title)| json!({
            "id": id,
            "title": title,
        })).collect::<Vec<Value>>(),
        "counts": { "left": left.len(), "right": right.len(), "blended": blended.len() },
    })))
}
