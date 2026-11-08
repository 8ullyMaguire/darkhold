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

use crate::auth::{MaybeSession, RequirePseud, RequireSession};
use crate::http::{ApiError, ApiResult};
use crate::routes::discovery::require_operator;
use crate::state::AppState;

/// A 422 with a sentence, in the shape the rest of the API answers with.
fn invalid(message: impl Into<String>) -> ApiError {
    ApiError(darkhold_domain::AppError::Validation {
        message: message.into(),
        field_errors: std::collections::BTreeMap::new(),
    })
}

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
    // M45-69: the operator's shelf, when one exists. It is published beside the computed vector rather
    // than replacing it in the record, and `source` says which one answered — a reader of this response
    // can tell "the operator wants this" from "our readers happen to like this".
    let operator = darkhold_db::operator_theme::get(state.db(), &instance_host)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;
    let theme = instance_theme::get_local_theme(state.db(), &instance_host)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;

    let source = darkhold_domain::operator_theme::precedence(
        scheduled.is_some(),
        operator.is_some(),
        theme.is_some(),
    );
    let effective = match (&scheduled, &operator, &theme) {
        (Some(schedule), _, _) => schedule.theme_vector.clone(),
        (None, Some(shelf), _) => shelf.theme_vector.clone(),
        (None, None, Some(computed)) => computed.theme_vector.clone(),
        (None, None, None) => serde_json::json!({}),
    };
    let theme = serde_json::json!({
        "instance_id": instance_host,
        "theme_vector": effective,
        "source": source.as_str(),
        "public": theme.as_ref().map(|t| t.public).unwrap_or(false),
        "computed_at": theme.as_ref().map(|t| t.computed_at.clone()),
        "updated_at": theme.as_ref().map(|t| t.updated_at.clone()),
        "schedule": scheduled.as_ref().map(schedule_json),
        // Visible, not blended: the stated shelf keeps its own top tags and seed counts, so an operator
        // can see what their shelf actually became.
        "operator_shelf": operator.as_ref().map(|shelf| serde_json::json!({
            "top_tags": shelf.top_tags,
            "seed_count": shelf.seed_count,
            "resolved_count": shelf.resolved_count,
            "sourced_at": shelf.sourced_at,
        })),
        // What the readers' bookmarks say, kept even when the shelf wins, so the difference is legible.
        "computed_vector": theme.as_ref().map(|t| t.theme_vector.clone()),
    });
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
    method: axum::http::Method,
    uri: axum::http::Uri,
    headers: axum::http::HeaderMap,
    body: String,
) -> ApiResult<Json<Value>> {
    // Signature first, and before the body is parsed: an unsigned request must not get the courtesy of a `422` that tells the
    // sender their JSON shape was wrong, because the shape is not what is wrong with it.
    let target = uri
        .path_and_query()
        .map_or_else(|| uri.path().to_owned(), |pq| pq.as_str().to_owned());
    let verdict = crate::http_signature::check(
        state.peer_keys(),
        method.as_str(),
        &target,
        &headers,
        body.as_bytes(),
    )
    .await;

    // A refusal is recorded, not only returned: an inbox under probe is a fact the operator should be able to see, and a 401
    // that leaves no trace makes "was anyone knocking" unanswerable.
    let refused = match &verdict {
        crate::http_signature::Verdict::Verified(key_id) => {
            tracing::info!(%key_id, "AP inbox: signature verified");
            None
        }
        crate::http_signature::Verdict::Refused(reason) => {
            tracing::warn!(%reason, "AP inbox: signature refused");
            Some(reason.clone())
        }
        crate::http_signature::Verdict::Unsigned => {
            if state.config().federation.require_signature {
                Some("no Signature header".to_owned())
            } else {
                None
            }
        }
    };
    if let Some(reason) = refused {
        let (kind, actor) = peek_identity(&body);
        if let Ok(id) =
            darkhold_db::external::record_inbound(state.db(), &peer_host_of(&actor), &kind, &actor)
                .await
        {
            if let Err(error) =
                darkhold_db::external::set_inbound_state(state.db(), &id, "rejected", Some(&reason))
                    .await
            {
                tracing::warn!(%error, "a refused intake row could not be marked rejected");
            }
        }
        // `AuthRequired` is the estate's 401: the request carried no credential this instance could verify.
        return Err(ApiError(darkhold_domain::AppError::AuthRequired));
    }

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

    // Every received activity is also an **intake row**, written `quarantined`. The activity is stored either way; what this
    // adds is the accounting the operator needs — who sent it, what type, when — and the honest starting state: nothing about
    // this request has been verified. Anybody can POST here, and until signatures are checked the ledger is the only record
    // that says so.
    let peer = peer_host_of(actor_id);
    match darkhold_db::external::record_inbound(
        state.db(),
        &peer,
        activity_type,
        obj_id.as_deref().unwrap_or(actor_id),
    )
    .await
    {
        Ok(id) => {
            // The state is the answer to "was this request verified": `accepted` is what a verified signature earns, and it is
            // not an operator's opinion — it is the one thing the instance can check for itself. An unsigned activity on an
            // instance that allows unsigned traffic stays `quarantined`, because nothing verified it.
            if let crate::http_signature::Verdict::Verified(key_id) = &verdict {
                if let Err(error) = darkhold_db::external::set_inbound_state(
                    state.db(),
                    &id,
                    "accepted",
                    Some(&format!("signature verified: {key_id}")),
                )
                .await
                {
                    tracing::warn!(%error, "a verified intake row could not be marked accepted");
                }
            }
        }
        Err(error) => {
            // The activity is already stored and the peer is waiting for an answer; a bookkeeping failure must not become a
            // refusal that makes them retry and leaves the activity written twice.
            tracing::warn!(%error, "an inbound activity could not be entered in the intake ledger");
        }
    }

    Ok(Json(serde_json::json!({ "status": "received" })))
}

/// The activity's type and actor, read from the body **without** deciding whether the body is acceptable.
///
/// Only used on the refusal path, where the point is to record what arrived: an unsigned request that could not be parsed still
/// names itself as `Unknown`, and a row saying "something refused arrived from this host" is more useful than no row at all.
fn peek_identity(body: &str) -> (String, String) {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return ("Unknown".to_owned(), String::new());
    };
    let kind = value
        .get("type")
        .and_then(|kind| kind.as_str())
        .unwrap_or("Unknown")
        .to_owned();
    let actor = value
        .get("actor")
        .and_then(|actor| actor.as_str())
        .unwrap_or_default()
        .to_owned();
    (kind, actor)
}

/// The host an activity came from, for the ledger's `peer_host`.
///
/// An actor id is a URL and the host is the part that identifies the peer. A missing or unparseable actor id is recorded as
/// `unknown` rather than refused: the row's job is to disclose that something unverified arrived, and a refusal here would
/// drop the evidence along with the claim.
fn peer_host_of(actor_id: &str) -> String {
    actor_id
        .split("//")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .filter(|host| !host.is_empty())
        .map(|host| host.to_ascii_lowercase())
        .unwrap_or_else(|| "unknown".to_owned())
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

/// The operator's view of the intake ledger: what arrived, from whom, and what is still undecided.
#[derive(Debug, serde::Deserialize)]
struct InboundQuery {
    /// `quarantined`, `accepted` or `rejected`. Absent means every state.
    state: Option<String>,
    limit: Option<i64>,
}

async fn list_inbound(
    State(state): State<AppState>,
    RequireSession(session): RequireSession,
    axum::extract::Query(query): axum::extract::Query<InboundQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    require_operator(&state, &session)?;
    if let Some(named) = query.state.as_deref() {
        if !matches!(named, "quarantined" | "accepted" | "rejected") {
            return Err(invalid(format!(
                "{named} is not an intake state; quarantined, accepted and rejected are"
            )));
        }
    }
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let rows = darkhold_db::external::inbound_recent(state.db(), query.state.as_deref(), limit)
        .await
        .map_err(|error| ApiError(darkhold_domain::AppError::Internal(error.into())))?;
    let waiting = rows.iter().filter(|row| row.state == "quarantined").count();
    Ok(Json(serde_json::json!({
        "records": rows,
        // The count is of the page, and says so: an operator deciding "is anything waiting" needs the number that belongs to
        // the rows in front of them, not a total assembled from a second query that could disagree with the first.
        "quarantined_in_page": waiting,
        "limit": limit,
    })))
}

#[derive(Debug, serde::Deserialize)]
struct DecideBody {
    /// `accepted` or `rejected`.
    state: String,
    /// Free text. Optional on acceptance, expected on rejection — but not required: a refusal that cannot be recorded without
    /// a sentence is a refusal an operator would make without recording it.
    note: Option<String>,
}

async fn decide_inbound(
    State(state): State<AppState>,
    RequireSession(session): RequireSession,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(body): Json<DecideBody>,
) -> ApiResult<Json<serde_json::Value>> {
    require_operator(&state, &session)?;
    if !matches!(body.state.as_str(), "accepted" | "rejected") {
        return Err(invalid(
            "an intake row is accepted or rejected; quarantined is where intake starts, not where a decision goes",
        ));
    }
    let changed = darkhold_db::external::set_inbound_state(
        state.db(),
        &id,
        &body.state,
        body.note.as_deref(),
    )
    .await
    .map_err(|error| ApiError(darkhold_domain::AppError::Internal(error.into())))?;
    if !changed {
        return Err(ApiError(darkhold_domain::AppError::NotFound {
            resource: "intake record",
        }));
    }
    Ok(Json(serde_json::json!({ "id": id, "state": body.state })))
}

/// This instance's public picks, as an ActivityStreams collection (§0.4).
///
/// Public and impersonal: the body is the same for everybody, because a collection a peer fetches must be the instance's answer
/// and not a snapshot of one reader's feed.
async fn ap_picks(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let document = crate::peer_picks::publish(&state)
        .await
        .map_err(|error| ApiError(darkhold_domain::AppError::Internal(error)))?;
    Ok(Json(document))
}

/// The operator's list of peer-picks subscriptions, with what the last fetch said.
async fn list_pick_subscriptions(
    State(state): State<AppState>,
    RequireSession(session): RequireSession,
) -> ApiResult<Json<Value>> {
    require_operator(&state, &session)?;
    let rows = darkhold_db::peer_picks::subscriptions(state.db())
        .await
        .map_err(|error| ApiError(darkhold_domain::AppError::Internal(error.into())))?;
    Ok(Json(serde_json::json!({ "subscriptions": rows })))
}

/// The instance directory (§0.4): who publishes what, searchable by one public topic.
///
/// Anonymous because the themes it searches are already public documents — a directory behind a login would be a directory only
/// operators can use, which is the opposite of what "sister instances find each other" means.
async fn instance_directory(
    State(state): State<AppState>,
    MaybeSession(_session): MaybeSession,
    Query(query): Query<TopicQuery>,
) -> ApiResult<Json<Value>> {
    let topic = query.topic.trim();
    if topic.is_empty() {
        return Err(invalid("a directory search names a topic"));
    }
    let hits = darkhold_db::instance_theme::search_public_by_topic(state.db(), topic)
        .await
        .map_err(|error| ApiError(darkhold_domain::AppError::Internal(error)))?;
    Ok(Json(serde_json::json!({
        "topic": topic,
        "instances": hits,
        "total": hits.len(),
    })))
}

#[derive(Debug, serde::Deserialize)]
struct TopicQuery {
    #[serde(default)]
    topic: String,
}

#[derive(Debug, serde::Deserialize)]
struct SubscribeBody {
    /// The peer's host, as it appears in its URLs. The identity of the subscription.
    peer_host: String,
    /// The collection document the sweep fetches.
    collection_url: String,
}

/// Subscribe to a peer's picks, or re-point an existing subscription for the same host.
async fn subscribe_picks(
    State(state): State<AppState>,
    RequireSession(session): RequireSession,
    Json(body): Json<SubscribeBody>,
) -> ApiResult<Json<Value>> {
    require_operator(&state, &session)?;
    let host = body.peer_host.trim().to_ascii_lowercase();
    let url = body.collection_url.trim().to_owned();
    if host.is_empty() || host.contains('/') || host.contains(' ') {
        return Err(invalid(
            "a peer host is a host name, without a scheme or a path",
        ));
    }
    // The URL must be the host it is filed under, and it must be https: a subscription is a request made on the operator's
    // behalf, and one filed under host A while pointing at host B is a way to make this instance fetch somewhere it did not say.
    let parsed = url
        .parse::<axum::http::Uri>()
        .map_err(|error| invalid(format!("the collection URL is not a URL: {error}")))?;
    if parsed.scheme_str() != Some("https") {
        return Err(invalid("a collection URL must be https"));
    }
    if parsed.host().map(str::to_ascii_lowercase).as_deref() != Some(host.as_str()) {
        return Err(invalid(format!(
            "a collection URL for {host} must be on {host}"
        )));
    }
    let id = darkhold_db::peer_picks::subscribe(state.db(), &host, &url)
        .await
        .map_err(|error| ApiError(darkhold_domain::AppError::Internal(error.into())))?;
    Ok(Json(serde_json::json!({
        "id": id,
        "peer_host": host,
        "collection_url": url,
    })))
}

/// Stop following a peer. The picks go with the subscription.
async fn unsubscribe_picks(
    State(state): State<AppState>,
    RequireSession(session): RequireSession,
    Path(host): Path<String>,
) -> ApiResult<Json<Value>> {
    require_operator(&state, &session)?;
    let removed = darkhold_db::peer_picks::unsubscribe(state.db(), &host)
        .await
        .map_err(|error| ApiError(darkhold_domain::AppError::Internal(error.into())))?;
    if !removed {
        return Err(ApiError(darkhold_domain::AppError::NotFound {
            resource: "peer picks subscription",
        }));
    }
    Ok(Json(
        serde_json::json!({ "peer_host": host, "removed": true }),
    ))
}

/// Recent picks from every enabled sister instance, attributed.
///
/// Public: a pick is a recommendation the peer published, and hiding it behind a session would make the feature invisible to the
/// readers it exists for.
async fn read_peer_picks(
    State(state): State<AppState>,
    MaybeSession(_session): MaybeSession,
    Query(query): Query<LimitQuery>,
) -> ApiResult<Json<Value>> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let picks = darkhold_db::peer_picks::recent_picks(state.db(), limit)
        .await
        .map_err(|error| ApiError(darkhold_domain::AppError::Internal(error.into())))?;
    Ok(Json(serde_json::json!({ "picks": picks, "limit": limit })))
}

#[derive(Debug, serde::Deserialize)]
struct LimitQuery {
    limit: Option<i64>,
}

pub fn routes() -> axum::Router<AppState> {
    axum::Router::new()
        // ActivityPub endpoints (public inbox, actor discovery)
        .route("/federation/inbox", post(ap_inbox))
        // §0.4 sister instances: the publishing half of "subscribe to each other's public picks".
        .route("/federation/picks", get(ap_picks))
        .route("/federation/directory", get(instance_directory))
        .route("/discovery/peer-picks", get(read_peer_picks))
        .route(
            "/operator/federation/picks",
            get(list_pick_subscriptions).put(subscribe_picks),
        )
        .route(
            "/operator/federation/picks/{host}",
            axum::routing::delete(unsubscribe_picks),
        )
        // The intake ledger's two doors: what arrived, and the operator's answer to it. A quarantine nothing can release is
        // not a policy, it is a table.
        .route("/operator/federation/inbound", get(list_inbound))
        .route(
            "/operator/federation/inbound/{id}/state",
            post(decide_inbound),
        )
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
        // M45-69 — what the instance is FOR, stated by pointing at a shelf.
        .route(
            "/federation/theme/operator",
            get(get_operator_theme)
                .post(set_operator_theme)
                .delete(delete_operator_theme),
        )
        // Public discovery
        .route("/federation/themes", get(list_public_themes))
        // Peers (admin)
        .route("/federation/peers", get(list_peers))
        .route("/federation/peers/similar", get(list_similar_peers))
        .route("/federation/peers", post(set_peer_state))
}

// ---------------------------------------------------------------------------
// M45-69 — operator taste: the shelf, published rather than blended
// ---------------------------------------------------------------------------

/// What the operator posted.
#[derive(Debug, Deserialize)]
pub struct OperatorShelfBody {
    /// One line per work: a catalogue title or id, a URL, or a work this instance does not have.
    pub seeds: Vec<String>,
}

/// `POST /api/v1/federation/theme/operator` — state what the instance is for.
///
/// Every line is resolved the same way `?seed=` resolves a seed: a catalogue row first, then stored
/// external evidence by id. What each line contributed is kept, including the ones that contributed
/// nothing — "3 of 5 seeds used" is the useful fact, and a response that only listed the three would be
/// indistinguishable from a shelf of three.
pub async fn set_operator_theme(
    State(state): State<AppState>,
    RequirePseud {
        user: _user,
        pseud_id,
    }: RequirePseud,
    Json(body): Json<OperatorShelfBody>,
) -> ApiResult<Json<Value>> {
    if pseud_id.to_string() != ADMIN_PSEUD_ID {
        return Err(ApiError(darkhold_domain::AppError::AccessDenied));
    }
    const MAX_SEEDS: usize = 50;
    let lines: Vec<&str> = body
        .seeds
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect();
    if lines.is_empty() {
        return Err(ApiError(darkhold_domain::AppError::field(
            "seeds",
            "name at least one work — an empty shelf says nothing about what the instance is for",
        )));
    }
    if lines.len() > MAX_SEEDS {
        return Err(ApiError(darkhold_domain::AppError::field(
            "seeds",
            format!(
                "{} works named, and a shelf takes at most {MAX_SEEDS}",
                lines.len()
            ),
        )));
    }

    let instance_host = state.config().site.base_url.clone();
    let mut seed_tags: Vec<darkhold_domain::operator_theme::SeedTags> = Vec::new();
    let mut record: Vec<Value> = Vec::new();

    for (index, line) in lines.iter().enumerate() {
        let number = index + 1;
        let classified = darkhold_domain::external_seed::classify(line);
        let spec = match classified {
            darkhold_domain::external_seed::Line::Blank => {
                record.push(serde_json::json!({
                    "line": number, "spec": line, "status": "unused",
                    "reason": "empty line",
                }));
                continue;
            }
            darkhold_domain::external_seed::Line::TooLong { chars } => {
                record.push(serde_json::json!({
                    "line": number, "spec": line, "status": "unused",
                    "reason": format!("{chars} characters is a pasted paragraph, not a title"),
                }));
                continue;
            }
            darkhold_domain::external_seed::Line::Pointer { spec, .. } => spec,
        };

        let tags = match darkhold_db::metrics::resolve_seed(state.db(), &spec).await {
            Ok(darkhold_db::metrics::SeedMatch::One(row)) => {
                let tags = darkhold_db::operator_theme::tags_for_item(state.db(), &row.id)
                    .await
                    .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;
                let (status, reason) = if tags.is_empty() {
                    (
                        "unused",
                        Some("in the catalogue, but carries no tags to state"),
                    )
                } else {
                    ("used", None)
                };
                record.push(serde_json::json!({
                    "line": number, "spec": spec, "status": status,
                    "title": row.title, "id": row.id, "tags": tags,
                    "reason": reason,
                }));
                tags
            }
            Ok(darkhold_db::metrics::SeedMatch::Many(rows)) => {
                record.push(serde_json::json!({
                    "line": number, "spec": spec, "status": "ambiguous",
                    "reason": format!("{} rows match and none is dominant — name one by id", rows.len()),
                    "candidates": rows.iter().take(6).map(|row| serde_json::json!({
                        "id": row.id, "title": row.title, "likes": row.likes,
                    })).collect::<Vec<_>>(),
                }));
                continue;
            }
            Ok(darkhold_db::metrics::SeedMatch::None) => {
                // Evidence this instance cannot read: no tags, and saying so is the point.
                let known = darkhold_db::external_seed::by_spec(
                    state.db(),
                    &_user.account_id.to_string(),
                    &spec,
                )
                .await
                .ok()
                .flatten();
                record.push(serde_json::json!({
                    "line": number, "spec": spec, "status": "unused",
                    "reason": "not in this instance's catalogue, so it carries no tags",
                    "external_id": known.map(|row| row.id),
                }));
                continue;
            }
            Err(error) => return Err(ApiError(darkhold_domain::AppError::Internal(error))),
        };

        if !tags.is_empty() {
            seed_tags.push(darkhold_domain::operator_theme::SeedTags {
                label: spec.clone(),
                tags,
            });
        }
    }

    let (vector, top_tags) = darkhold_domain::operator_theme::from_seeds(&seed_tags);
    if vector.is_empty() {
        return Err(ApiError(darkhold_domain::AppError::field(
            "seeds",
            format!(
                "theme: none of the {} seeds carries a tag — a shelf with no tags states nothing this \
                 instance can act on",
                lines.len()
            ),
        )));
    }

    let now = darkhold_db::identity::now_rfc3339();
    let claim = darkhold_db::operator_theme::ThemeClaim {
        vector: &serde_json::json!(vector),
        top_tags: &serde_json::json!(top_tags
            .iter()
            .map(|(tag, weight)| serde_json::json!({
                "tag": tag, "weight": weight,
            }))
            .collect::<Vec<_>>()),
        seeds: &Value::Array(record.clone()),
        seed_count: lines.len() as i64,
        resolved_count: seed_tags.len() as i64,
    };
    darkhold_db::operator_theme::upsert(state.db(), &instance_host, &claim, &now)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;

    Ok(Json(serde_json::json!({
        "instance_id": instance_host,
        "theme_vector": vector,
        "top_tags": claim.top_tags,
        "seeds": record,
        "seed_count": lines.len(),
        "resolved_count": seed_tags.len(),
        "sourced_at": now,
        "updated_at": now,
        "note": "published on its own: this is what the instance is FOR, not what its readers happen to like — \
                 the computed theme is untouched",
    })))
}

/// `GET /api/v1/federation/theme/operator` — the stated shelf, or `null`.
pub async fn get_operator_theme(
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
    let theme = darkhold_db::operator_theme::get(state.db(), &instance_host)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;
    Ok(Json(serde_json::json!({ "operator_theme": theme })))
}

/// `DELETE /api/v1/federation/theme/operator` — forget the shelf, returning to bookmarks.
pub async fn delete_operator_theme(
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
    let removed = darkhold_db::operator_theme::delete(state.db(), &instance_host)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;
    let now = darkhold_db::identity::now_rfc3339();
    // What answers *now* after forgetting the shelf — which includes a season that was already open.
    // Answering `none` while an event is running would be a small lie in a response whose whole job is
    // to say where the theme comes from.
    let scheduled = darkhold_db::theme_schedule::active_at(state.db(), &instance_host, &now)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?
        .is_some();
    let computed = darkhold_db::instance_theme::get_local_theme(state.db(), &instance_host)
        .await
        .ok()
        .flatten()
        .is_some();
    Ok(Json(serde_json::json!({
        "removed": removed,
        "source": darkhold_domain::operator_theme::precedence(scheduled, false, computed).as_str(),
        "updated_at": now,
    })))
}
