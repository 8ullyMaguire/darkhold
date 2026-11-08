//! Taste health (§0.4.1 admin taste profile, §16.19 health layer, M17 phase 2) — the module's doors.
//!
//! `crates/db/src/taste_health.rs` had five functions with no caller: the per-work vector's producer, its writer, its
//! reader, and the two halves of the taste probe. The consequence was not cosmetic — `work_taste_vectors` was a table
//! nothing could fill, so every reader of a vector got `[]` and no error anywhere said why. A cached vector that is never
//! written reads exactly like a work nobody has taste in common with.
//!
//! Four doors, and the rule each keeps:
//!
//! * **The vector is a diagnosis, not a publication.** A work's vector is a function of its tags *and* the operator's
//!   taste profile, so publishing it per work would let anybody with enough works invert the profile one coordinate at a
//!   time. These are `Operator` routes for that reason — the same reason the profile itself is.
//! * **`[]` means three different things and the door says which.** No profile on this instance ("unavailable"), a
//!   profile but no vector yet ("computed" — the read computes it lazily and stores it), a stored vector ("cached"). A
//!   bare `[]` would claim "this work shares nothing with the instance's taste", which is a claim nobody made.
//! * **Stale is the operator's call, not a silent recompute.** A tag edit does not touch `work_taste_vectors`, so the
//!   read serves what is stored and `POST` is the refresh. Recomputing on every read would make a hot work pay for its
//!   popularity, and recomputing on edit needs a hook in the tag editor that does not exist — so the refresh is explicit
//!   and named, rather than hoped for.
//! * **A probe is the reader's own, one per work, and the vocabulary is the schema's.** `taste_probes` CHECKs
//!   `positive | negative | neutral` (migration 0054) and is `UNIQUE (account_id, work_id)` — a second answer to the same
//!   probe replaces the first rather than stacking, which is what the store's upsert already says and what the door must
//!   not contradict by accepting a word the table will reject with a 500.

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use darkhold_db::taste_health;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::{MaybeSession, RequireSession};
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

/// The vocabulary `taste_probes.engagement` accepts, in one place.
///
/// It is a copy of the migration's CHECK on purpose: the table is the authority, and a word that is not in this list
/// would otherwise reach the database and come back as a 500 — a client error reported as a server fault.
const ENGAGEMENTS: [&str; 3] = ["positive", "negative", "neutral"];

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/admin/works/{id}/taste-vector",
            get(admin_get_vector).post(admin_recompute_vector),
        )
        .route("/me/taste/probes", get(list_my_probes))
        .route("/me/taste/probes/{work_id}", post(record_my_probe))
}

fn internal(error: sqlx::Error) -> ApiError {
    ApiError(darkhold_domain::AppError::Internal(error.into()))
}

/// The work's taste vector, computed on first ask.
///
/// `state` is the part that matters: `cached` (a stored vector), `computed` (there was a profile and no vector, so this
/// call derived and stored one), `unavailable` (this instance has no admin taste profile, so there is nothing to derive
/// from — every work answers the same way and the operator needs to know it is a configuration fact, not a data one).
async fn admin_get_vector(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(work_id): Path<String>,
) -> ApiResult<Json<Value>> {
    // One decision, one place (ADR 0003): the operator predicate lives in admin_scope, not in a fourteenth copy.
    crate::routes::admin_scope::require_estate_hidden(&state, &user)?;

    let cached = taste_health::get_work_vector(state.db(), &work_id)
        .await
        .map_err(internal)?;
    if !cached.is_empty() {
        let dimensions = taste_health::get_admin_taste_profile(state.db())
            .await
            .map_err(internal)?
            .len();
        return Ok(Json(json!({
            "work_id": work_id,
            "state": "cached",
            "dimensions": dimensions,
            "vector": cached,
        })));
    }

    let computed = taste_health::compute_and_store_work_vector(state.db(), &work_id)
        .await
        .map_err(internal)?;
    if computed.is_empty() {
        return Ok(Json(json!({
            "work_id": work_id,
            "state": "unavailable",
            "dimensions": 0,
            "vector": [],
            "reason": "this instance has no admin taste profile, so no work has a vector",
        })));
    }
    Ok(Json(json!({
        "work_id": work_id,
        "state": "computed",
        "dimensions": computed.len(),
        "vector": computed,
    })))
}

/// Recompute and store, discarding whatever is cached.
///
/// This is the only path that refreshes a vector, because tags can change after it was computed and nothing in the tag
/// editor tells this module. Named as a button rather than a timer: the binary has no scheduler, and a vector that is
/// recomputed on every read is a hot work paying for its popularity.
async fn admin_recompute_vector(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(work_id): Path<String>,
) -> ApiResult<Json<Value>> {
    // One decision, one place (ADR 0003): the operator predicate lives in admin_scope, not in a fourteenth copy.
    crate::routes::admin_scope::require_estate_hidden(&state, &user)?;

    let computed = taste_health::compute_and_store_work_vector(state.db(), &work_id)
        .await
        .map_err(internal)?;
    if computed.is_empty() {
        return Ok(Json(json!({
            "work_id": work_id,
            "state": "unavailable",
            "dimensions": 0,
            "vector": [],
            "reason": "this instance has no admin taste profile, so no work has a vector",
        })));
    }
    Ok(Json(json!({
        "work_id": work_id,
        "state": "computed",
        "dimensions": computed.len(),
        "vector": computed,
    })))
}

fn account_of(user: Option<crate::auth::SessionUser>) -> ApiResult<String> {
    let account = user.map(|u| u.account_id.to_string()).unwrap_or_default();
    if account.is_empty() {
        return Err(ApiError(darkhold_domain::AppError::field(
            "session",
            "sign in to record a taste probe",
        )));
    }
    Ok(account)
}

#[derive(Debug, Deserialize)]
pub struct ProbeBody {
    /// `positive`, `negative` or `neutral` — the words `taste_probes` accepts.
    pub engagement: String,
}

/// One answer to one probe: an upsert on `(account, work)`, so answering again replaces rather than stacks.
async fn record_my_probe(
    State(state): State<AppState>,
    MaybeSession(user): MaybeSession,
    Path(work_id): Path<String>,
    Json(body): Json<ProbeBody>,
) -> ApiResult<Json<Value>> {
    let account = account_of(user)?;
    let engagement = body.engagement.trim();
    if !ENGAGEMENTS.contains(&engagement) {
        return Err(ApiError(darkhold_domain::AppError::field(
            "engagement",
            &format!("one of {}", ENGAGEMENTS.join(", ")),
        )));
    }

    taste_health::record_probe_engagement(state.db(), &account, &work_id, engagement)
        .await
        .map_err(internal)?;

    Ok(Json(json!({
        "work_id": work_id,
        // The upsert's shape, said out loud: a probe has one answer per (reader, work), and this one is it.
        "engagement": engagement,
        "replaced": true,
    })))
}

/// Every probe this reader has answered, newest conclusion attached.
async fn list_my_probes(
    State(state): State<AppState>,
    MaybeSession(user): MaybeSession,
) -> ApiResult<Json<Value>> {
    let account = account_of(user)?;
    let rows = taste_health::get_probe_engagements(state.db(), &account)
        .await
        .map_err(internal)?;
    let probes: Vec<Value> = rows
        .into_iter()
        .map(|(work_id, engagement)| json!({"work_id": work_id, "engagement": engagement}))
        .collect();
    Ok(Json(json!({"count": probes.len(), "probes": probes})))
}
