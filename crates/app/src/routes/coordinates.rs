//! Work coordinates over HTTP — the surface §49.3's measures have never had (M70-13, for M45-14).
//!
//! ## Why this file exists
//!
//! `darkhold_db::work_coordinates` stores four prose measures per work — sentence-length variance, dialogue ratio,
//! vocabulary richness, unit-length spread — and the module's own header explains why they are stored rather than
//! computed per page: §49.7 says the same text must give the same coordinates, and a live computation would let a
//! work move its own ranking mid-session with no visible edit. Nine store functions were written for that; until this
//! file, **none of them had a caller or a route anywhere in `crates/app`**, while M45-14 was recorded as
//! `implemented-fully-tested`. Tested and unreachable is the exact shape this ratchet exists to catch, so the end is a
//! surface rather than a deletion: the feature is real, it just had nobody to answer to.
//!
//! ## The two doors, and why the split is the design
//!
//! * `GET /works/{id}/coordinates` — the work's owner, or an operator. It answers "does this work have coordinates,
//!   which text version were they measured on, and if none, why". §49.3's rule that an absent coordinate is not a
//!   zero only means something if the *reason* is visible: `unmeasurable` carries `no_text` or `too_short` rather
//!   than a null a reader would read as "flat prose".
//! * `POST /admin/works/{id}/coordinates` — operator only. A recompute is a backfill, and the store freezes the answer
//!   so that a recompute is an auditable event rather than a side effect of somebody loading a page. `text_version` is
//!   read from the work *at measure time*, so a stored row always says which revision its numbers describe.
//!
//! A stranger gets `404`, not `403`, on the read: whether a work exists and is yours is public, but whether this
//! account authored a work it may not read is not something a refusal should confirm (ADR 0030).

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

use darkhold_domain::coordinates::{Coordinates, Unmeasurable};
use darkhold_domain::{AppError, WorkId};

use crate::auth::RequireSession;
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/works/{id}/coordinates", get(read_coordinates))
        .route("/admin/works/{id}/coordinates", post(recompute))
        .route("/admin/coordinates/recompute", post(recompute_stale))
}

/// The work's owner, or the instance's answer to a stranger.
///
/// The same rule the cast and tag doors use: a claim about a work belongs to whoever wrote it. An operator is not the
/// owner and is not silently treated as one — it reaches this door through the operator branch, which is a different
/// question with a different answer.
async fn require_owner(state: &AppState, work_id: &str, account: &str) -> ApiResult<()> {
    let owned = darkhold_db::orphan::owned_by(state.db(), work_id, account)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
    if !owned {
        tracing::debug!(work = %work_id, account, "a non-owner asked for a work's coordinates");
        return Err(ApiError(AppError::NotFound { resource: "work" }));
    }
    Ok(())
}

/// What the store knows about one work, in the shape a reader can act on.
///
/// Three states, and they are three different facts rather than one `null`:
///
/// * never measured — no row at all;
/// * measured and unmeasurable — a row carrying the reason, which is strictly more useful than an absence;
/// * measured — the four measures, the two counts, and the text version they describe.
fn view(work_id: &str, stored: Option<darkhold_db::work_coordinates::StoredCoordinates>) -> Value {
    let Some(stored) = stored else {
        return json!({
            "work_id": work_id,
            "measured": false,
            "reason": "never measured",
            "unmeasurable": Value::Null,
            "coordinates": Value::Null,
            "text_version": Value::Null,
            "computed_at": Value::Null,
        });
    };

    match &stored.coordinates {
        Coordinates::Measured(c) => json!({
            "work_id": work_id,
            "measured": true,
            "reason": Value::Null,
            "unmeasurable": Value::Null,
            "coordinates": {
                "sentence_length_variance": c.sentence_length_variance,
                "dialogue_ratio": c.dialogue_ratio,
                "vocabulary_richness": c.vocabulary_richness,
                "unit_length_spread": c.unit_length_spread,
                "word_count": c.word_count,
                "sentence_count": c.sentence_count,
            },
            "text_version": stored.text_version,
            "computed_at": stored.computed_at,
        }),
        Coordinates::Unmeasurable(reason) => json!({
            "work_id": work_id,
            "measured": false,
            "reason": reason.explain(),
            "unmeasurable": match reason {
                Unmeasurable::NoText => "no_text",
                Unmeasurable::TooShort => "too_short",
            },
            "coordinates": Value::Null,
            "text_version": stored.text_version,
            "computed_at": stored.computed_at,
        }),
    }
}

async fn read_coordinates(
    State(state): State<AppState>,
    session: RequireSession,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let account = session.0.account_id.to_string();
    if crate::routes::discovery::require_operator(&state, &session.0).is_err() {
        require_owner(&state, &id, &account).await?;
    }
    let stored = darkhold_db::work_coordinates::stored_coordinates(state.db(), &id)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
    Ok(Json(view(&id, stored)))
}

/// Recompute and store, for an operator.
///
/// The answer is the same document the read door gives, so an operator who recomputes sees what a caller will see and
/// does not need a second request to find out what they just wrote. `null` coordinates with `never measured` back
/// means the work has no text at all — not that the write failed.
async fn recompute(
    State(state): State<AppState>,
    session: RequireSession,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    crate::routes::discovery::require_operator(&state, &session.0)?;
    let work_id: WorkId = id
        .parse()
        .map_err(|_| ApiError(AppError::NotFound { resource: "work" }))?;
    let text_version = darkhold_db::content::current_work_version(state.db(), work_id)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
    let computed_at = crate::format_rfc3339(time::OffsetDateTime::now_utc());
    darkhold_db::work_coordinates::measure_work_and_store(
        state.db(),
        &id,
        text_version,
        &computed_at,
    )
    .await
    .map_err(|error| ApiError(AppError::Internal(error)))?;
    let stored = darkhold_db::work_coordinates::stored_coordinates(state.db(), &id)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
    Ok(Json(view(&id, stored)))
}

/// Re-measure the works that already have a row, oldest question first — the backfill door.
///
/// `stored_work_ids` is the enumeration this exists for: without it an operator can only recompute a work they already
/// know about, which is not a backfill. `stale_only` is the default and the honest one — a row whose `text_version`
/// still matches the work has nothing to recompute, and re-deriving it would spend the work's reading time to write the
/// same numbers back. `limit` bounds one request (default 50, at most 500): the catalogue is 58,042 works, and an
/// endpoint that tries to walk all of them in one request is an endpoint that times out half-done and leaves the
/// operator guessing how far it got.
///
/// The answer reports what happened rather than a bare `ok`: `scanned` is what the limit allowed, `remaining` is how
/// much the limit left behind, and `no_text` is a work with no revisions at all — a fact about the work, not a failure
/// of this request.
async fn recompute_stale(
    State(state): State<AppState>,
    session: RequireSession,
    Json(body): Json<Value>,
) -> ApiResult<Json<Value>> {
    crate::routes::discovery::require_operator(&state, &session.0)?;

    let limit = body["limit"].as_u64().unwrap_or(50).clamp(1, 500) as usize;
    let stale_only = body["stale_only"].as_bool().unwrap_or(true);

    let all = darkhold_db::work_coordinates::stored_work_ids(state.db())
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;

    let mut scanned = 0usize;
    let mut skipped_current = 0usize;
    let mut recomputed = 0usize;
    let mut measured = 0usize;
    let mut unmeasurable = 0usize;
    let mut no_text = 0usize;

    for work in &all {
        if scanned >= limit {
            break;
        }
        scanned += 1;
        let Ok(work_id) = work.parse::<WorkId>() else {
            continue;
        };
        let current = darkhold_db::content::current_work_version(state.db(), work_id)
            .await
            .map_err(|error| ApiError(AppError::Internal(error)))?;
        if stale_only {
            let stored = darkhold_db::work_coordinates::stored_coordinates(state.db(), work)
                .await
                .map_err(|error| ApiError(AppError::Internal(error)))?;
            if stored.is_some_and(|s| s.text_version == current) {
                skipped_current += 1;
                continue;
            }
        }
        let computed_at = crate::format_rfc3339(time::OffsetDateTime::now_utc());
        let outcome = darkhold_db::work_coordinates::measure_work_and_store(
            state.db(),
            work,
            current,
            &computed_at,
        )
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
        match outcome {
            None => no_text += 1,
            Some(stored) => {
                recomputed += 1;
                match stored.coordinates {
                    Coordinates::Measured(_) => measured += 1,
                    Coordinates::Unmeasurable(_) => unmeasurable += 1,
                }
            }
        }
    }

    Ok(Json(json!({
        "scanned": scanned,
        "recomputed": recomputed,
        "measured": measured,
        "unmeasurable": unmeasurable,
        "no_text": no_text,
        "skipped_current": skipped_current,
        "remaining": all.len().saturating_sub(scanned),
        "stale_only": stale_only,
    })))
}
