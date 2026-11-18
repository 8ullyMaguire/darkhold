//! Row 413 — reading lists over HTTP, on gravity's imported user-list service.
//!
//! ```text
//! POST   /reading-lists                              create a list
//! GET    /reading-lists/{id}                         read one list (404 if unreadable)
//! PATCH  /reading-lists/{id}                         rename / change visibility
//! DELETE /reading-lists/{id}                         delete the list and its children
//! GET    /me/reading-lists                           lists owned by the caller
//! POST   /reading-lists/{id}/items                   add a work
//! GET    /reading-lists/{id}/items                   read the works
//! DELETE /reading-lists/{id}/items/{work}            remove one work
//! POST   /reading-lists/{id}/collaborators           grant curation rights
//! GET    /reading-lists/{id}/collaborators           read the collaborators
//! DELETE /reading-lists/{id}/collaborators/{user}    revoke one collaborator
//! ```
//!
//! There are no permission branches here. Every door authenticates the caller
//! and hands the account id to the service, which owns visibility and mutation
//! rules. A private list that the caller may not read returns the same 404 as a
//! list that does not exist.

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use darkhold_db::user_list;
use darkhold_domain::error::AppError;
use darkhold_domain::user_list::{
    AddUserListCollaboratorRequest, AddUserListItemRequest, CreateUserListRequest,
    UpdateUserListRequest,
};

use crate::auth::{RequireSession, SessionUser};
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

/// The reading-list routes, mounted in the `library` module.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/reading-lists", post(create))
        .route("/me/reading-lists", get(mine))
        .route(
            "/reading-lists/{id}",
            get(get_one).patch(update).delete(delete_one),
        )
        .route("/reading-lists/{id}/items", post(add_item).get(list_items))
        .route(
            "/reading-lists/{id}/items/{work}",
            axum::routing::delete(remove_item),
        )
        .route(
            "/reading-lists/{id}/collaborators",
            post(add_collaborator).get(list_collaborators),
        )
        .route(
            "/reading-lists/{id}/collaborators/{user}",
            axum::routing::delete(remove_collaborator),
        )
}

#[derive(Debug, Deserialize)]
struct LimitQuery {
    limit: Option<i64>,
}

fn limit_of(query: &LimitQuery) -> i64 {
    query.limit.unwrap_or(100)
}

fn parse_id(value: &str, noun: &str) -> Result<String, ApiError> {
    uuid::Uuid::parse_str(value)
        .map(|_| value.to_owned())
        .map_err(|_| {
            ApiError(AppError::Validation {
                message: format!("{noun} id must be a UUID"),
                field_errors: Default::default(),
            })
        })
}

fn actor(user: &SessionUser) -> String {
    user.account_id.to_string()
}

async fn create(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Json(req): Json<CreateUserListRequest>,
) -> ApiResult<Json<Value>> {
    let list = user_list::create(state.db(), &actor(&user), &req).await?;
    Ok(Json(json!({ "list": list })))
}

async fn mine(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Query(query): Query<LimitQuery>,
) -> ApiResult<Json<Value>> {
    let id = actor(&user);
    let lists = user_list::list_by_owner(state.db(), &id, Some(&id), limit_of(&query)).await?;
    Ok(Json(json!({ "lists": lists })))
}

async fn get_one(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let id = parse_id(&id, "list")?;
    let viewer = actor(&user);
    let view = user_list::get_visible(state.db(), &id, Some(&viewer))
        .await?
        .ok_or(ApiError(AppError::NotFound {
            resource: "reading list",
        }))?;
    // One `list` object everywhere: create/update return the row, so a read
    // must not hide it behind a second nesting level. The service's read-side
    // extras are merged into the same object rather than losing their keys.
    let mut list = serde_json::to_value(&view.list).expect("list row serialises");
    if let Some(object) = list.as_object_mut() {
        object.insert("item_count".to_owned(), json!(view.item_count));
        object.insert("is_owner".to_owned(), json!(view.is_owner));
    }
    Ok(Json(json!({ "list": list })))
}

async fn update(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
    Json(req): Json<UpdateUserListRequest>,
) -> ApiResult<Json<Value>> {
    let id = parse_id(&id, "list")?;
    let list = user_list::update(state.db(), &id, &actor(&user), &req).await?;
    Ok(Json(json!({ "list": list })))
}

async fn delete_one(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let id = parse_id(&id, "list")?;
    user_list::delete(state.db(), &id, &actor(&user)).await?;
    Ok(Json(json!({ "deleted": true, "id": id })))
}

async fn add_item(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
    Json(req): Json<AddUserListItemRequest>,
) -> ApiResult<Json<Value>> {
    let id = parse_id(&id, "list")?;
    let work = parse_id(&req.work_id, "work")?;
    let mut req = req;
    req.work_id = work;
    let item = user_list::add_item(state.db(), &id, &actor(&user), &req).await?;
    Ok(Json(json!({ "item": item })))
}

async fn list_items(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
    Query(query): Query<LimitQuery>,
) -> ApiResult<Json<Value>> {
    let id = parse_id(&id, "list")?;
    let viewer = actor(&user);
    let items = user_list::items(state.db(), &id, Some(&viewer), limit_of(&query)).await?;
    Ok(Json(json!({ "items": items })))
}

async fn remove_item(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path((id, work)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let id = parse_id(&id, "list")?;
    let work = parse_id(&work, "work")?;
    user_list::remove_item(state.db(), &id, &actor(&user), &work).await?;
    Ok(Json(json!({ "removed": true, "work_id": work })))
}

async fn add_collaborator(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
    Json(req): Json<AddUserListCollaboratorRequest>,
) -> ApiResult<Json<Value>> {
    let id = parse_id(&id, "list")?;
    let who = parse_id(&req.user_id, "account")?;
    let mut req = req;
    req.user_id = who;
    let collaborator = user_list::add_collaborator(state.db(), &id, &actor(&user), &req).await?;
    Ok(Json(json!({ "collaborator": collaborator })))
}

async fn list_collaborators(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
    Query(query): Query<LimitQuery>,
) -> ApiResult<Json<Value>> {
    let id = parse_id(&id, "list")?;
    let viewer = actor(&user);
    let collaborators =
        user_list::collaborators(state.db(), &id, Some(&viewer), limit_of(&query)).await?;
    Ok(Json(json!({ "collaborators": collaborators })))
}

async fn remove_collaborator(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path((id, who)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let id = parse_id(&id, "list")?;
    let who = parse_id(&who, "account")?;
    user_list::remove_collaborator(state.db(), &id, &actor(&user), &who).await?;
    Ok(Json(json!({ "removed": true, "user_id": who })))
}
