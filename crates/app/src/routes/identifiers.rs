//! M45-37 — stable identifiers over HTTP.
//!
//! ```text
//! GET    /works/:id/identifiers              what this work claims (public)
//! PUT    /works/:id/identifiers/:scheme      claim one (a contributor)
//! DELETE /works/:id/identifiers/:scheme/:value  drop one (a contributor)
//! GET    /identifiers/lookup?scheme=&value=  which work is this? (public)
//! ```
//!
//! # Why the read side is public and the write side is not
//!
//! The decision recorded on M45-37 is that an identifier on a work is a **public catalogue claim**: it is how
//! two rows are recognised as the same book, so hiding it would make the recognition unverifiable by the people
//! it serves. Writing one is a claim *about someone's work*, so it needs a contributor on that work.
//!
//! # A conflict is an answer
//!
//! `PUT` on an identifier another work already holds returns `409`-shaped information in a validation answer
//! that names the holder. The row's whole point is that one identifier names one thing; reporting "already
//! claimed by work X" is the useful result, and silently overwriting would move a claim off somebody's work.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use commons_identify::Scheme;
use darkhold_db::identifier::{self as store, Attached, Target};

use crate::auth::{MaybeSession, RequireSession, SessionUser};
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;
use darkhold_domain::error::AppError;

/// The identifier routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/works/{id}/identifiers", get(for_work))
        .route("/works/{id}/identifiers/{scheme}", put(claim))
        .route("/works/{id}/identifiers/{scheme}/{value}", delete(drop_one))
        .route("/identifiers/lookup", get(lookup))
}

fn invalid(message: impl Into<String>) -> ApiError {
    ApiError(AppError::Validation {
        message: message.into(),
        field_errors: std::collections::BTreeMap::new(),
    })
}

fn identifier_json(row: &store::Identifier) -> Value {
    json!({
        "scheme": row.scheme.as_str(),
        "value": row.value,
        "work_id": row.work_id,
        "edition_id": row.edition_id,
        "source": row.source,
        "created_at": row.created_at,
    })
}

async fn for_work(
    State(state): State<AppState>,
    MaybeSession(_session): MaybeSession,
    Path(work): Path<String>,
) -> ApiResult<Json<Value>> {
    let rows = store::for_work(state.db(), &work).await?;
    Ok(Json(json!({
        "items": rows.iter().map(identifier_json).collect::<Vec<_>>(),
    })))
}

#[derive(Debug, Deserialize)]
struct LookupParams {
    scheme: Option<String>,
    value: Option<String>,
}

async fn lookup(
    State(state): State<AppState>,
    MaybeSession(_session): MaybeSession,
    Query(params): Query<LookupParams>,
) -> ApiResult<Json<Value>> {
    let scheme_raw = params
        .scheme
        .as_deref()
        .ok_or_else(|| invalid("scheme: name one of isbn, tmdb, musicbrainz, isrc, wikidata"))?;
    let scheme = Scheme::parse(scheme_raw).ok_or_else(|| {
        invalid(format!(
            "scheme: {scheme_raw} is not a known identifier scheme"
        ))
    })?;
    let value = params
        .value
        .as_deref()
        .ok_or_else(|| invalid("value: the identifier to look up"))?;

    match store::lookup(state.db(), scheme, value).await? {
        Some(row) => Ok(Json(json!({
            "found": true,
            "identifier": identifier_json(&row),
        }))),
        // Not an error: "nobody has claimed this yet" is the answer a caller needs, and a 404 here would
        // make a lookup that found nothing indistinguishable from a lookup that failed.
        None => Ok(Json(json!({
            "found": false,
            "scheme": scheme.as_str(),
            "value": value.trim(),
        }))),
    }
}

#[derive(Debug, Deserialize)]
struct ClaimBody {
    value: String,
    /// `imported`, `curator`, `quorum` or `author`. Defaults to `curator`.
    source: Option<String>,
}

async fn claim(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path((work, scheme_raw)): Path<(String, String)>,
    Json(body): Json<ClaimBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let scheme = Scheme::parse(&scheme_raw).ok_or_else(|| {
        invalid(format!(
            "scheme: {scheme_raw} is not a known identifier scheme"
        ))
    })?;
    let source = body.source.as_deref().unwrap_or("curator");
    if !matches!(source, "imported" | "curator" | "quorum" | "author") {
        return Err(invalid(format!(
            "source: {source} is not imported, curator, quorum or author"
        )));
    }
    require_contributor(&state, &user, &work).await?;

    let target = Target::Work(work.clone());
    match store::attach(state.db(), scheme, &body.value, &target, source).await? {
        Attached::Stored(row) => Ok((
            StatusCode::CREATED,
            Json(json!({ "identifier": identifier_json(&row) })),
        )),
        Attached::Taken(holder) => {
            let where_held = holder
                .work_id
                .clone()
                .or_else(|| holder.edition_id.clone())
                .unwrap_or_else(|| "another target".to_owned());
            Err(invalid(format!(
                "{} {} is already claimed by {where_held} — an identifier names one thing",
                scheme.as_str(),
                holder.value
            )))
        }
        Attached::Refused(reason) => Err(invalid(reason)),
    }
}

async fn drop_one(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path((work, scheme_raw, value)): Path<(String, String, String)>,
) -> ApiResult<StatusCode> {
    let scheme = Scheme::parse(&scheme_raw).ok_or_else(|| {
        invalid(format!(
            "scheme: {scheme_raw} is not a known identifier scheme"
        ))
    })?;
    require_contributor(&state, &user, &work).await?;
    let target = Target::Work(work);
    if store::detach(state.db(), scheme, &value, &target).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        // "this work does not claim it" — including when another work does, which this route must not touch.
        Err(ApiError(AppError::NotFound {
            resource: "identifier on this work",
        }))
    }
}

/// Refuse a caller who is not a contributor on the work, without confirming that the work exists.
async fn require_contributor(
    state: &AppState,
    user: &SessionUser,
    raw_work: &str,
) -> ApiResult<()> {
    let Some(pseud) = user.pseud_id else {
        return Err(ApiError(AppError::AccessDenied));
    };
    let work_id = raw_work
        .parse::<darkhold_domain::ids::WorkId>()
        .map_err(|_| ApiError(AppError::NotFound { resource: "work" }))?;
    if darkhold_db::content::find_work(state.db(), work_id)
        .await?
        .is_none()
    {
        return Err(ApiError(AppError::NotFound { resource: "work" }));
    }
    let contributors = darkhold_db::collaboration::contributors_for_work(state.db(), work_id)
        .await
        .unwrap_or_default();
    if contributors.iter().any(|row| row.pseud_id == pseud) {
        Ok(())
    } else {
        Err(ApiError(AppError::NotFound { resource: "work" }))
    }
}
