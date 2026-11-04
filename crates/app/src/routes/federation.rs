//! Federation routes: instance theme management, peers, AP actors, inbox/outbox.
//!
//! Instance themes are PRIVATE by default. Users may opt-in to make them
//! public for better instance discovery. Other instances can publish their
//! themes via the public endpoint for cross-instance discovery.

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::Json;
use serde::Deserialize;
use serde_json::Value;

use darkhold_db::federation as fed;
use darkhold_db::instance_theme;

use crate::auth::{MaybeSession, RequirePseud};
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

const ADMIN_PSEUD_ID: &str = "9aa50758-bf73-44d3-bf65-9fddb4135925";

// ---------------------------------------------------------------------------
// Query types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ThemeVisibility {
    #[serde(default)]
    pub public: bool,
}

#[derive(Debug, Deserialize)]
pub struct SimilarQuery {
    #[serde(default = "default_threshold")]
    pub threshold: f64,
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_threshold() -> f64 {
    30.0
}
fn default_limit() -> i64 {
    50
}

// ---------------------------------------------------------------------------
// Instance theme management (admin only)
// ---------------------------------------------------------------------------

/// Get current instance theme vector and visibility setting (admin only).
pub async fn get_instance_theme(
    State(state): State<AppState>,
    RequirePseud {
        user: _user,
        pseud_id,
    }: RequirePseud,
) -> ApiResult<Json<Value>> {
    if pseud_id.to_string() != ADMIN_PSEUD_ID {
        return Err(ApiError(darkhold_domain::AppError::AccessDenied));
    }
    let instance_host = state.config().site.base_url.clone();
    // M45-50: a scheduled window, when one covers now, is this instance's gravity for the moment. The
    // instance's own vector is left untouched, so the event ends by itself and nothing has to be
    // edited back at midnight.
    let now = darkhold_db::identity::now_rfc3339();
    let scheduled = darkhold_db::theme_schedule::active_at(state.db(), &instance_host, &now)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;
    let theme = instance_theme::get_local_theme(state.db(), &instance_host)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?
        .map(|t| {
            serde_json::json!({
                "instance_id": t.instance_id,
                "theme_vector": match &scheduled {
                    Some(schedule) => schedule.theme_vector.clone(),
                    None => t.theme_vector.clone(),
                },
                "public": t.public,
                "computed_at": t.computed_at,
                "updated_at": t.updated_at,
                "schedule": scheduled.as_ref().map(schedule_json),
            })
        })
        .unwrap_or(serde_json::json!({
            "instance_id": instance_host,
            "theme_vector": {},
            "public": false,
            "computed_at": null,
            "updated_at": null,
        }));
    Ok(Json(theme))
}

/// Recompute theme vector from user bookmarks and update (admin only).
pub async fn recompute_theme(
    State(state): State<AppState>,
    RequirePseud {
        user: _user,
        pseud_id,
    }: RequirePseud,
) -> ApiResult<Json<Value>> {
    if pseud_id.to_string() != ADMIN_PSEUD_ID {
        return Err(ApiError(darkhold_domain::AppError::AccessDenied));
    }
    let instance_host = state.config().site.base_url.clone();

    // Aggregate tag weights from bookmarks and private_tags
    let theme_vector = compute_theme_from_engagement(state.db())
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;

    // Keep existing visibility setting (default false)
    let existing = instance_theme::get_local_theme(state.db(), &instance_host)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;
    let is_public = existing.as_ref().map(|t| t.public).unwrap_or(false);

    let now = darkhold_db::identity::now_rfc3339();
    instance_theme::upsert_theme(state.db(), &instance_host, &theme_vector, is_public, &now)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;

    Ok(Json(serde_json::json!({
        "status": "recomputed",
        "theme_vector": theme_vector,
        "public": is_public,
        "updated_at": now,
    })))
}

/// Set theme visibility (admin only).
pub async fn set_theme_visibility(
    State(state): State<AppState>,
    RequirePseud {
        user: _user,
        pseud_id,
    }: RequirePseud,
    Json(body): Json<ThemeVisibility>,
) -> ApiResult<Json<Value>> {
    if pseud_id.to_string() != ADMIN_PSEUD_ID {
        return Err(ApiError(darkhold_domain::AppError::AccessDenied));
    }
    let instance_host = state.config().site.base_url.clone();
    let existing = instance_theme::get_local_theme(state.db(), &instance_host)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;

    let tv = existing
        .as_ref()
        .map(|t| t.theme_vector.clone())
        .unwrap_or_else(|| serde_json::json!({}));
    let now = darkhold_db::identity::now_rfc3339();
    instance_theme::upsert_theme(state.db(), &instance_host, &tv, body.public, &now)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;

    Ok(Json(serde_json::json!({
        "public": body.public,
        "updated_at": now,
    })))
}

/// Compute theme vector from all engagement signals: bookmarks, private_tags, reading_status.
async fn compute_theme_from_engagement(db: &darkhold_db::Database) -> anyhow::Result<Value> {
    let mut weights: std::collections::HashMap<String, f64> = std::collections::HashMap::new();

    // Tags from bookmarks (weight 3.0 each)
    let rows: Vec<(String,)> = match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_as(
            "SELECT tn.canonical
             FROM bookmarks b
             JOIN work_tags wt ON wt.work_id = CAST(b.subject_id AS INTEGER)
             JOIN canonical_entities tn ON tn.id = wt.node_id",
        )
        .fetch_all(db.sqlite_pool().expect("sqlite"))
        .await
        .unwrap_or_default(),
        darkhold_db::Backend::Postgres => sqlx::query_as(
            "SELECT tn.canonical
             FROM bookmarks b
             JOIN work_tags wt ON wt.work_id = b.subject_id
             JOIN canonical_entities tn ON tn.id = wt.node_id",
        )
        .fetch_all(db.postgres_pool().expect("postgres"))
        .await
        .unwrap_or_default(),
    };
    for (tag,) in rows {
        *weights.entry(tag).or_default() += 3.0;
    }

    // Tags from private_tags (weight 2.0 each)
    let rows: Vec<(String,)> = match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_as(
            "SELECT tn.canonical
             FROM private_tags pt
             JOIN work_tags wt ON wt.work_id = CAST(pt.subject_id AS INTEGER)
             JOIN canonical_entities tn ON tn.id = wt.node_id",
        )
        .fetch_all(db.sqlite_pool().expect("sqlite"))
        .await
        .unwrap_or_default(),
        darkhold_db::Backend::Postgres => sqlx::query_as(
            "SELECT tn.canonical
             FROM private_tags pt
             JOIN work_tags wt ON wt.work_id = pt.subject_id
             JOIN canonical_entities tn ON tn.id = wt.node_id",
        )
        .fetch_all(db.postgres_pool().expect("postgres"))
        .await
        .unwrap_or_default(),
    };
    for (tag,) in rows {
        *weights.entry(tag).or_default() += 2.0;
    }

    // Tags from reading_status=completed (weight 1.0 each)
    let rows: Vec<(String,)> = match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_as(
            "SELECT tn.canonical
             FROM reading_status rs
             JOIN work_tags wt ON wt.work_id = CAST(rs.subject_id AS INTEGER)
             JOIN canonical_entities tn ON tn.id = wt.node_id
             WHERE rs.status = 'complete' AND rs.subject_type = 'work'",
        )
        .fetch_all(db.sqlite_pool().expect("sqlite"))
        .await
        .unwrap_or_default(),
        darkhold_db::Backend::Postgres => sqlx::query_as(
            "SELECT tn.canonical
             FROM reading_status rs
             JOIN work_tags wt ON wt.work_id = rs.subject_id
             JOIN canonical_entities tn ON tn.id = wt.node_id
             WHERE rs.status = 'complete' AND rs.subject_type = 'work'",
        )
        .fetch_all(db.postgres_pool().expect("postgres"))
        .await
        .unwrap_or_default(),
    };
    for (tag,) in rows {
        *weights.entry(tag).or_default() += 1.0;
    }

    // Sort by weight desc, take top 100
    let mut sorted: Vec<(String, f64)> = weights.into_iter().collect();
    sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    sorted.truncate(100);

    let mut map = serde_json::Map::new();
    for (tag, weight) in sorted {
        map.insert(tag, serde_json::json!(weight));
    }
    Ok(Value::Object(map))
}

// ---------------------------------------------------------------------------
// Public instance discovery (anyone)
// ---------------------------------------------------------------------------

/// List all public instance themes (for discovery).
pub async fn list_public_themes(
    State(state): State<AppState>,
    MaybeSession(_session): MaybeSession,
) -> ApiResult<Json<Value>> {
    let themes = instance_theme::list_public_themes(state.db())
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;
    Ok(Json(serde_json::json!(themes)))
}

// ---------------------------------------------------------------------------
// Federation peers (admin only)
// ---------------------------------------------------------------------------

/// List all known federation peers with similarity scores (admin only).
pub async fn list_peers(
    State(state): State<AppState>,
    RequirePseud {
        user: _user,
        pseud_id,
    }: RequirePseud,
) -> ApiResult<Json<Value>> {
    if pseud_id.to_string() != ADMIN_PSEUD_ID {
        return Err(ApiError(darkhold_domain::AppError::AccessDenied));
    }
    let peers = fed::list_all_peers(state.db())
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;
    Ok(Json(serde_json::json!(peers)))
}

/// List similar peers above a threshold (admin only).
pub async fn list_similar_peers(
    State(state): State<AppState>,
    RequirePseud {
        user: _user,
        pseud_id,
    }: RequirePseud,
    Query(query): Query<SimilarQuery>,
) -> ApiResult<Json<Value>> {
    if pseud_id.to_string() != ADMIN_PSEUD_ID {
        return Err(ApiError(darkhold_domain::AppError::AccessDenied));
    }
    let peers = fed::list_similar_peers(state.db(), query.threshold, query.limit)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;
    Ok(Json(serde_json::json!(peers)))
}

/// Set a peer's federation state (admin only).
#[derive(Debug, Deserialize)]
pub struct SetPeerState {
    pub peer_host: String,
    pub state: String,
    pub auto_federate: bool,
}

pub async fn set_peer_state(
    State(state): State<AppState>,
    RequirePseud {
        user: _user,
        pseud_id,
    }: RequirePseud,
    Json(body): Json<SetPeerState>,
) -> ApiResult<Json<Value>> {
    if pseud_id.to_string() != ADMIN_PSEUD_ID {
        return Err(ApiError(darkhold_domain::AppError::AccessDenied));
    }
    fed::upsert_peer(
        state.db(),
        &body.peer_host,
        0.0,
        &body.state,
        body.auto_federate,
        Some("admin"),
    )
    .await
    .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;
    Ok(Json(serde_json::json!({ "status": "ok" })))
}

// ---------------------------------------------------------------------------
// ActivityPub inbox (public — other instances send here)
// ---------------------------------------------------------------------------

/// ActivityPub inbox — receive activities from other instances.
pub async fn ap_inbox(
    State(state): State<AppState>,
    MaybeSession(_session): MaybeSession,
    body: String,
) -> ApiResult<Json<Value>> {
    let activity: Value = serde_json::from_str(&body)
        .map_err(|_| ApiError(darkhold_domain::AppError::field("body", "invalid JSON")))?;

    let activity_type = activity
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown");
    let actor_id = activity.get("actor").and_then(|v| v.as_str()).unwrap_or("");

    tracing::info!("AP inbox: {} from {}", activity_type, actor_id);

    // Look up or create the remote actor
    let actor = fed::get_actor_by_ap_id(state.db(), actor_id)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;

    let actor_db_id = match actor {
        Some(a) => a.id,
        None => {
            let inbox = format!("{}/inbox", actor_id);
            fed::create_actor(
                state.db(),
                "service",
                None,
                Some(actor_id),
                actor_id,
                &inbox,
                &format!("{}/outbox", actor_id),
                None,
                None,
                "unknown",
            )
            .await
            .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?
        }
    };

    // Store activity
    let obj_id = activity
        .get("object")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    fed::create_activity(
        state.db(),
        activity_type,
        &actor_db_id,
        obj_id.as_deref(),
        Some("Activity"),
        &activity,
    )
    .await
    .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;

    Ok(Json(serde_json::json!({ "status": "received" })))
}

// ---------------------------------------------------------------------------
// ActivityPub actor discovery
// ---------------------------------------------------------------------------

/// Get local actor profile.
pub async fn ap_actor(
    State(state): State<AppState>,
    MaybeSession(_session): MaybeSession,
) -> ApiResult<Json<Value>> {
    let base = &state.config().site.base_url;
    Ok(Json(serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": format!("{}/federation/actor", base),
        "type": "Application",
        "name": state.config().site.name.clone(),
        "inbox": format!("{}/federation/inbox", base),
        "outbox": format!("{}/federation/outbox", base),
        "publicKey": {
            "id": format!("{}/federation/actor#main-key", base),
            "owner": format!("{}/federation/actor", base),
            "publicKeyPem": crate::keys::load_public_key_pem()
        }
    })))
}

/// ActivityPub outbox — list outgoing activities.
pub async fn ap_outbox(
    State(state): State<AppState>,
    RequirePseud {
        user: _user,
        pseud_id,
    }: RequirePseud,
) -> ApiResult<Json<Value>> {
    if pseud_id.to_string() != ADMIN_PSEUD_ID {
        return Err(ApiError(darkhold_domain::AppError::AccessDenied));
    }
    let base = &state.config().site.base_url;
    Ok(Json(serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": format!("{}/federation/outbox", base),
        "type": "OrderedCollection",
        "totalItems": 0,
        "orderedItems": []
    })))
}

// ---------------------------------------------------------------------------
// M45-50 — scheduled gravity: a taste vector that applies for a window
// ---------------------------------------------------------------------------

/// One schedule as the API writes it.
fn schedule_json(schedule: &darkhold_db::theme_schedule::ThemeSchedule) -> Value {
    serde_json::json!({
        "id": schedule.id,
        "label": schedule.label,
        "theme_vector": schedule.theme_vector,
        "starts_at": schedule.starts_at,
        "ends_at": schedule.ends_at,
        "created_at": schedule.created_at,
        "revoked_at": schedule.revoked_at,
    })
}

/// The `POST` body.
#[derive(Debug, serde::Deserialize)]
pub struct ScheduleBody {
    label: String,
    theme_vector: Value,
    starts_at: String,
    ends_at: String,
}

/// `GET /api/v1/federation/theme/schedule` — every window this instance wrote (admin only).
pub async fn list_theme_schedules(
    State(state): State<AppState>,
    RequirePseud {
        user: _user,
        pseud_id,
    }: RequirePseud,
) -> ApiResult<Json<Value>> {
    if pseud_id.to_string() != ADMIN_PSEUD_ID {
        return Err(ApiError(darkhold_domain::AppError::AccessDenied));
    }
    let instance_host = state.config().site.base_url.clone();
    let schedules = darkhold_db::theme_schedule::list_for_instance(state.db(), &instance_host)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;
    Ok(Json(serde_json::json!({
        "items": schedules.iter().map(schedule_json).collect::<Vec<_>>(),
    })))
}

/// `POST /api/v1/federation/theme/schedule` — write a window (admin only).
///
/// The window is validated by the domain rule before it reaches the store, so the operator reads one
/// sentence whether the mistake is a local timestamp or a window that ends before it starts.
pub async fn create_theme_schedule(
    State(state): State<AppState>,
    RequirePseud {
        user: _user,
        pseud_id,
    }: RequirePseud,
    Json(body): Json<ScheduleBody>,
) -> ApiResult<(axum::http::StatusCode, Json<Value>)> {
    if pseud_id.to_string() != ADMIN_PSEUD_ID {
        return Err(ApiError(darkhold_domain::AppError::AccessDenied));
    }
    let window = darkhold_domain::theme_window::parse_window(&body.starts_at, &body.ends_at)
        .map_err(|message| ApiError(darkhold_domain::AppError::field("window", message)))?;
    if body.label.trim().is_empty() {
        return Err(ApiError(darkhold_domain::AppError::field(
            "label",
            "a schedule needs a label: it is what a reader of the theme response is shown when the \
             seasonal vector applies",
        )));
    }
    let instance_host = state.config().site.base_url.clone();
    let now = darkhold_db::identity::now_rfc3339();
    let id = darkhold_db::theme_schedule::create(
        state.db(),
        &instance_host,
        body.label.trim(),
        &body.theme_vector,
        &window.starts_at,
        &window.ends_at,
        &now,
    )
    .await
    .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(serde_json::json!({
            "id": id,
            "label": body.label.trim(),
            "starts_at": window.starts_at,
            "ends_at": window.ends_at,
            "created_at": now,
        })),
    ))
}

/// `DELETE /api/v1/federation/theme/schedule/{id}` — call a window off (admin only).
///
/// Revoking keeps the row: an event that was cancelled is a fact worth reading back, and the same
/// reason a saved query keeps its runs.
pub async fn revoke_theme_schedule(
    State(state): State<AppState>,
    RequirePseud {
        user: _user,
        pseud_id,
    }: RequirePseud,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    if pseud_id.to_string() != ADMIN_PSEUD_ID {
        return Err(ApiError(darkhold_domain::AppError::AccessDenied));
    }
    let instance_host = state.config().site.base_url.clone();
    let now = darkhold_db::identity::now_rfc3339();
    let revoked = darkhold_db::theme_schedule::revoke(state.db(), &instance_host, &id, &now)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;
    if !revoked {
        return Err(ApiError(darkhold_domain::AppError::NotFound {
            resource: "theme schedule",
        }));
    }
    Ok(Json(serde_json::json!({ "id": id, "revoked_at": now })))
}

pub fn routes() -> axum::Router<AppState> {
    axum::Router::new()
        // ActivityPub endpoints (public inbox, actor discovery)
        .route("/federation/inbox", post(ap_inbox))
        .route("/federation/actor", get(ap_actor))
        .route("/federation/outbox", get(ap_outbox))
        // Instance theme management (admin)
        .route(
            "/federation/theme",
            get(get_instance_theme).put(set_theme_visibility),
        )
        .route("/federation/theme/recompute", post(recompute_theme))
        // M45-50 — time-boxed gravity.
        .route(
            "/federation/theme/schedule",
            get(list_theme_schedules).post(create_theme_schedule),
        )
        .route(
            "/federation/theme/schedule/{id}",
            axum::routing::delete(revoke_theme_schedule),
        )
        // Public discovery
        .route("/federation/themes", get(list_public_themes))
        // Peers (admin)
        .route("/federation/peers", get(list_peers))
        .route("/federation/peers/similar", get(list_similar_peers))
        .route("/federation/peers", post(set_peer_state))
}
