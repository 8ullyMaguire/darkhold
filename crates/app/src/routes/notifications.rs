//! §5.5/§12 — the notifications inbox API.
//!
//! Read-only plus mark-read; rows are written by the surfaces that produce
//! them (forum replies, sales, gifts). Everything here is session-scoped:
//! an account sees only its own inbox.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde_json::json;

use crate::auth::RequireSession;
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

/// GET /notifications — the caller's inbox, newest first, with an unread count.
///
/// W4 task 3's adapter, in the shape the wave document asks for: **keep the route, proxy the inbox, map the
/// fields**. The estate has one inbox now; this product had its own table, and the table stays as history and as
/// the fallback for the wave's rollback rule (`inbox = local` restores the local bell without a deploy).
///
/// The mapping is the part that matters: an estate event is `{type, site, object…}`, and darkhold's own UI
/// expects `{id, kind, title, body, read, created_at, work_id}`. Doing that conversion here — in one function,
/// once — is the difference between five adapters and five dialects.
async fn list_notifications(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<serde_json::Value>> {
    let account_id = user.account_id.to_string();
    // The estate's name for this person, not this product's.
    let subject = crate::inbox::canonical(state.db(), &account_id).await;

    if let Some(body) = crate::inbox::list(&subject, 200).await {
        let items = map_inbox_rows(&body);
        let unread = items
            .iter()
            .filter(|item| !item["read"].as_bool().unwrap_or(true))
            .count();
        return Ok(Json(
            json!({ "items": items, "unread_count": unread, "source": "estate" }),
        ));
    }

    // Fallback: the local table. A bell that goes empty because a service is restarting is a bell that lies.
    tracing::warn!("the estate inbox did not answer; serving this product's local history");
    let viewer_pseud: Option<uuid::Uuid> = Some(
        user.pseud_id
            .as_ref()
            .map(|p| p.as_uuid())
            .unwrap_or_else(|| user.account_id.as_uuid()),
    );
    let rules = darkhold_db::search::content_filter_sql::for_pseud(state.db(), viewer_pseud)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;
    let (rows, unread) = tokio::try_join!(
        darkhold_db::notifications::list_filtered(state.db(), &account_id, 200, &rules),
        darkhold_db::notifications::unread_count_filtered(state.db(), &account_id, &rules),
    )
    .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;
    let items: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|n| {
            json!({
                "id": n.id,
                "kind": n.kind,
                "title": n.title,
                "body": n.body,
                "read": !n.unread,
                "created_at": n.created_at,
                "work_id": n.work_id,
            })
        })
        .collect();
    Ok(Json(
        json!({ "items": items, "unread_count": unread, "source": "local" }),
    ))
}

/// The estate row, in this product's words.
///
/// `work_id` is only filled when the event is about a work: a forum reply has no work, and a UI that underlined
/// a null work id would make every notification look broken.
fn map_inbox_rows(body: &serde_json::Value) -> Vec<serde_json::Value> {
    body.get("data")
        .and_then(|data| data.as_array())
        .map(|rows| {
            rows.iter()
                .map(|row| {
                    let kind = row
                        .get("event_type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("event");
                    let site = row.get("site").and_then(|v| v.as_str()).unwrap_or("");
                    let object_kind = row
                        .get("object_kind")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let title = row
                        .get("object_title")
                        .and_then(|v| v.as_str())
                        .filter(|t| !t.is_empty())
                        .map(str::to_string)
                        .unwrap_or_else(|| headline(kind, site));
                    let body_text = row
                        .get("object_excerpt")
                        .and_then(|v| v.as_str())
                        .filter(|t| !t.is_empty())
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("on {site}"));
                    json!({
                        "id": row.get("id"),
                        "kind": kind,
                        "title": title,
                        "body": body_text,
                        "read": row.get("read_at").map(|v| !v.is_null()).unwrap_or(false),
                        "created_at": row.get("created_at"),
                        "work_id": if object_kind == "work" {
                            row.get("object_id").cloned().unwrap_or(serde_json::Value::Null)
                        } else {
                            serde_json::Value::Null
                        },
                        "site": site,
                        "url": row.get("object_url"),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// What to say when the emitter sent no title. A notification that reads "event" teaches nobody anything.
fn headline(kind: &str, site: &str) -> String {
    match kind {
        "comment" => "Someone commented".to_string(),
        "reply" => "Someone replied".to_string(),
        "mention" => "You were mentioned".to_string(),
        "kudos" => "Someone left kudos".to_string(),
        _ => format!("Activity on {site}"),
    }
}

/// POST /notifications/read-all — idempotent.
async fn read_all(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<StatusCode> {
    let account_id = user.account_id.to_string();
    if crate::inbox::mark_all_read(&crate::inbox::canonical(state.db(), &account_id).await)
        .await
        .is_ok()
    {
        return Ok(StatusCode::NO_CONTENT);
    }
    // The fallback also clears the local rows, so the two stores cannot disagree about what the person has seen.
    darkhold_db::notifications::mark_all_read(state.db(), &account_id)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /notifications/{id}/read — idempotent: an unknown id or an
/// already-read entry both settle to 204 rather than advertising existence.
async fn mark_read(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    // The id is a uuid column on both dialects; refusing a malformed id here
    // keeps the answer identical (404) instead of SQLite 204 / PostgreSQL 500.
    let id = id.parse::<uuid::Uuid>().map_err(|_| {
        ApiError(darkhold_domain::AppError::NotFound {
            resource: "notification",
        })
    })?;
    let account_id = user.account_id.to_string();
    if crate::inbox::mark_read(
        &crate::inbox::canonical(state.db(), &account_id).await,
        &[id.to_string()],
    )
    .await
    {
        return Ok(StatusCode::NO_CONTENT);
    }
    darkhold_db::notifications::mark_read(state.db(), &account_id, &id.to_string())
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;
    Ok(StatusCode::NO_CONTENT)
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/notifications", get(list_notifications))
        .route("/notifications/read-all", post(read_all))
        .route("/notifications/{id}/read", post(mark_read))
}
