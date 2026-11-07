//! M69-05 — mirroring by taste similarity, with the mirror attributed on the work page.
//!
//! * `GET  /api/v1/operator/mirroring/eligible` — who is close enough to hold a copy, and why the others are not.
//! * `POST /api/v1/operator/mirroring/{work_id}/mirrors` — record a peer as holding a copy.
//! * `POST /api/v1/operator/mirroring/mirrors/{id}/retire` — it was lost, or withdrawn.
//! * `GET  /api/v1/works/{work_id}/mirrors` — what a work page prints.
//!
//! # The eligible list is computed, not configured
//!
//! It comes from the fingerprints peers published and the one this instance publishes (`well_known::current`, the same
//! assembly the published document uses). An operator does not type a peer id and hope: the door re-computes the decision at
//! the moment of the request, so a mirror to a peer whose taste has since drifted away is refused with the number.
//!
//! # The attribution door is public, and closed by default
//!
//! A work page is read by strangers, so the mirror list is anonymous. That is also why it waits behind
//! `[discovery] work_mirroring`: saying "this copy is preserved elsewhere" is a claim about the instance's own work, and a
//! default-on claim is one nobody chose.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::RequireSession;
use crate::http::{ApiError, ApiResult};
use crate::routes::discovery::require_operator;
use crate::routes::well_known;
use crate::state::AppState;
use darkhold_domain::error::AppError;
use darkhold_domain::mirroring::{eligible, eligible_peers, Attribution, Ineligible, MIN_MIRROR_SIMILARITY};

/// The mirroring routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/operator/mirroring/eligible", get(eligible_peers_handler))
        .route("/operator/mirroring/{work_id}/mirrors", post(record))
        .route("/operator/mirroring/mirrors/{id}/retire", post(retire))
        .route("/works/{work_id}/mirrors", get(attribution))
}

fn closed(state: &AppState) -> Option<ApiError> {
    (!state.config().discovery.work_mirroring).then(|| {
        ApiError(AppError::NotFound {
            resource: "mirroring",
        })
    })
}

fn internal(error: anyhow::Error) -> ApiError {
    ApiError(AppError::Internal(error))
}

/// Parse a request body, answering `422` with the field error rather than an opaque rejection.
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

/// A live work's title. `published` narrows it to what a stranger may see.
///
/// The operator doors ask without the narrow filter — an operator prepares a mirror before a work is out — and the public
/// attribution door asks with it, because a mirror line is a disclosure about where a work lives and a draft's address is
/// not a stranger's to learn. The public door answers 404 for a draft, not an empty list: "no mirrors" and "not for you" are
/// different facts and only one of them is safe to tell.
async fn work_title(state: &AppState, work_id: &str, published: bool) -> anyhow::Result<Option<String>> {
    let filter = if published {
        " AND lifecycle = 'published' AND visibility = 'public'"
    } else {
        ""
    };
    let sqlite_sql =
        format!("SELECT title FROM works WHERE id = ? AND deleted_at IS NULL{filter}");
    let postgres_sql =
        format!("SELECT title FROM works WHERE id::text = $1 AND deleted_at IS NULL{filter}");
    let sql = state.db().sql(&sqlite_sql, &postgres_sql);
    let title: Option<String> = match state.db().backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query_scalar(&sql)
                .bind(work_id)
                .fetch_optional(state.db().sqlite_pool().expect("sqlite"))
                .await?
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query_scalar(&sql)
                .bind(work_id)
                .fetch_optional(state.db().postgres_pool().expect("postgres"))
                .await?
        }
    };
    Ok(title)
}

/// The configured floor, raised to the minimum the domain insists on.
fn floor(state: &AppState) -> f64 {
    state
        .config()
        .discovery
        .mirror_floor
        .unwrap_or(MIN_MIRROR_SIMILARITY)
        .max(MIN_MIRROR_SIMILARITY)
}

async fn eligible_peers_handler(
    State(state): State<AppState>,
    session: RequireSession,
) -> ApiResult<Json<Value>> {
    if let Some(error) = closed(&state) {
        return Err(error);
    }
    require_operator(&state, &session.0)?;

    let own = well_known::current(&state).await?;
    let peers = darkhold_db::peer_fingerprints::list(state.db())
        .await
        .map_err(internal)?;
    let floor = floor(&state);

    let Some(own) = own else {
        // No published fingerprint means no comparison is possible. Said plainly rather than answering an empty list, which
        // an operator would read as "no peer is close enough" — a different and false claim.
        return Ok(Json(json!({
            "own": Value::Null,
            "floor": floor,
            "eligible": [],
            "ineligible": [],
            "reason": "this instance has not published a taste fingerprint, so there is nothing to compare a peer against; \
                       set [discovery] enforced_rules or publish a theme and try again",
        })));
    };

    let documents: Vec<_> = peers.iter().map(|(fingerprint, _)| fingerprint.clone()).collect();
    let eligible_list = eligible_peers(&own, &documents, floor);
    let ineligible: Vec<Value> = documents
        .iter()
        .filter(|peer| !eligible_list.iter().any(|found| found.instance_id == peer.instance_id))
        .map(|peer| {
            let (reason, similarity) = match eligible(&own, peer) {
                Ok(found) => (
                    format!(
                        "similarity {:.3} clears the domain floor but not this instance's floor of {floor:.2}",
                        found.similarity
                    ),
                    Some(found.similarity),
                ),
                Err(why) => {
                    let measured = match &why {
                        Ineligible::TooDistant { similarity } => Some(*similarity),
                        _ => None,
                    };
                    (why.reason(), measured)
                }
            };
            json!({
                "instance_id": peer.instance_id,
                "reason": reason,
                "similarity": similarity,
            })
        })
        .collect();

    Ok(Json(json!({
        "own": own.instance_id,
        "floor": floor,
        "min_floor": MIN_MIRROR_SIMILARITY,
        "eligible": eligible_list.iter().map(|peer| json!({
            "instance_id": peer.instance_id,
            "similarity": peer.similarity,
            "similarity_bp": (peer.similarity * 10_000.0).round() as i64,
        })).collect::<Vec<Value>>(),
        "ineligible": ineligible,
    })))
}

#[derive(Debug, Deserialize)]
struct RecordRequest {
    /// The peer, by the id its fingerprint declares.
    peer_instance: String,
    /// Where a reader finds the copy there. Optional: a peer with no address is still a peer.
    #[serde(default)]
    peer_url: String,
}

async fn record(
    State(state): State<AppState>,
    session: RequireSession,
    Path(work_id): Path<String>,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    // The body is parsed *after* the gate, not by the extractor before it: a closed door has to answer 404 to every request,
    // and an extractor that runs first would answer 422 for a malformed body — telling a caller the route exists and reads
    // bodies, which is exactly what "the feature is not there" must not say.
    if let Some(error) = closed(&state) {
        return Err(error);
    }
    require_operator(&state, &session.0)?;
    let request: RecordRequest = parse(&body)?;

    // The work has to exist: a mirror row pointing at nothing is a line on no page.
    let Some(title) = work_title(&state, &work_id, false).await.map_err(internal)? else {
        return Err(ApiError(AppError::NotFound { resource: "work" }));
    };

    let own = well_known::current(&state).await?;
    let Some(own) = own else {
        return Err(validation(
            "this instance has not published a taste fingerprint, so no peer can be checked against it".to_owned(),
        ));
    };
    let peers = darkhold_db::peer_fingerprints::list(state.db())
        .await
        .map_err(internal)?;
    let document = peers
        .iter()
        .map(|(fingerprint, _)| fingerprint)
        .find(|fingerprint| fingerprint.instance_id == request.peer_instance);

    let Some(document) = document else {
        return Err(validation(format!(
            "no fingerprint from {} has been recorded; record one with PUT /operator/peering/fingerprints first",
            request.peer_instance
        )));
    };
    let floor = floor(&state);
    let decision = eligible(&own, document).map_err(|why| validation(why.reason()))?;
    if decision.similarity < floor {
        return Err(validation(format!(
            "taste similarity {:.3} is below this instance's mirror floor of {floor:.2}",
            decision.similarity
        )));
    }

    let now = darkhold_db::identity::now_rfc3339();
    let similarity_bp = (decision.similarity * 10_000.0).round() as i64;
    let mirror = darkhold_db::work_mirrors::record(
        state.db(),
        &work_id,
        &request.peer_instance,
        request.peer_url.trim(),
        similarity_bp,
        &now,
    )
    .await
    .map_err(internal)?;

    Ok(Json(json!({
        "mirror": {
            "id": mirror.id,
            "work_id": mirror.work_id,
            "title": title,
            "peer_instance": mirror.peer_instance,
            "peer_url": mirror.peer_url,
            "similarity": decision.similarity,
            "similarity_bp": mirror.similarity_bp,
            "state": mirror.state,
            "mirrored_at": mirror.mirrored_at,
        },
        "attribution": Attribution {
            peer: mirror.peer_instance.clone(),
            peer_url: mirror.peer_url.clone(),
            mirrored_at: mirror.mirrored_at.clone(),
        }.sentence(),
    })))
}

#[derive(Debug, Deserialize)]
struct RetireRequest {
    /// `lost` or `withdrawn`.
    state: String,
    reason: String,
}

async fn retire(
    State(state): State<AppState>,
    session: RequireSession,
    Path(id): Path<String>,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    if let Some(error) = closed(&state) {
        return Err(error);
    }
    require_operator(&state, &session.0)?;
    let request: RetireRequest = parse(&body)?;
    let now = darkhold_db::identity::now_rfc3339();
    let changed = darkhold_db::work_mirrors::retire(
        state.db(),
        &id,
        request.state.trim(),
        request.reason.trim(),
        &now,
    )
    .await
    .map_err(|error| validation(error.to_string()))?;
    if !changed {
        return Err(ApiError(AppError::NotFound {
            resource: "active mirror",
        }));
    }
    Ok(Json(json!({
        "retired": id,
        "state": request.state.trim(),
        "at": now,
        "note": "the row stays: a reader who saw the attribution yesterday is owed an explanation, not silence",
    })))
}

async fn attribution(
    State(state): State<AppState>,
    Path(work_id): Path<String>,
) -> ApiResult<Json<Value>> {
    if let Some(error) = closed(&state) {
        return Err(error);
    }
    // The same visibility rule the operator door uses, so the two cannot disagree about which works have a public mirror
    // line: a draft's mirrors are not a stranger's to learn, and the answer for one is 404, not an empty list.
    if work_title(&state, &work_id, true).await.map_err(internal)?.is_none() {
        return Err(ApiError(AppError::NotFound { resource: "work" }));
    }
    let rows = darkhold_db::work_mirrors::for_work(state.db(), &work_id)
        .await
        .map_err(internal)?;

    // The active copies are what the page says; retired ones are reported so a page can explain itself, but they are not
    // sentences a reader is shown by default.
    let lines: Vec<String> = rows
        .iter()
        .filter(|row| row.state == "active")
        .map(|row| {
            Attribution {
                peer: row.peer_instance.clone(),
                peer_url: row.peer_url.clone(),
                mirrored_at: row.mirrored_at.clone(),
            }
            .sentence()
        })
        .collect();

    Ok(Json(json!({
        "work_id": work_id,
        "active": rows.iter().filter(|row| row.state == "active").count(),
        "attribution": lines,
        "mirrors": rows.iter().map(|row| json!({
            "peer_instance": row.peer_instance,
            "peer_url": row.peer_url,
            "state": row.state,
            "mirrored_at": row.mirrored_at,
            "retired_at": row.retired_at,
            "retire_reason": row.retire_reason,
        })).collect::<Vec<Value>>(),
    })))
}
