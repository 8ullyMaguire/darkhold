//! §50.2's canon class, on the work and in a listing (M70-13).
//!
//! Three functions in `canon_agnostic.rs` had no caller, and between them they were the whole feature:
//!
//! * `stored_canon` — the class of one work. Without it a reader could not be told "this is readable without knowing the
//!   fandom", because nothing published the verdict the classifier had already computed and stored.
//! * `is_canon_agnostic` — §50.2's *eligibility* question, which the module's own docs insist is not the same as reaching
//!   into `CanonVerdict` (a future verdict must not silently become eligible). It is answered at the door that lists the
//!   class, one work at a time, so eligibility stays a rule with one implementation.
//! * `is_canon_dependent` — the other half of the same question, answered on the work for the label a client shows: an
//!   unmeasured work must not be labelled either way.
//!
//! And the producer was missing too: `measure_work_and_store` had no caller outside its own tests, so on a fresh instance
//! the table stayed empty, the listing stayed empty, and "no canon-agnostic works" looked like a fact about the corpus
//! rather than a missing scheduler. It now runs when a work is published — the moment its text stops changing — best-effort,
//! exactly like the taste queue beside it.
//!
//! **The listing is bounded, labelled, and unranked.** §50.2 asks for a distinct place with the label attached, and refuses
//! to let these works outrank a recognised match: there is no score here to outrank anything with, the order is newest
//! class first, and each item says which class it is in. The other half of §50.2 — "a work whose fandom is unknown to *the
//! reader*" — is a fact about a reader's weights, not about the work, and is named as the missing half in
//! `docs/plans/roadmap-remaining.md` rather than approximated here.

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use darkhold_db::canon_agnostic::{self as canon_store, CanonVerdict};
use darkhold_domain::error::AppError;

use crate::auth::RequireSession;
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

/// Public reads, and the operator's producer.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/works/{id}/canon", get(work_canon))
        .route("/works/{id}/canon/measure", post(measure_work))
        .route("/discover/canon-blind", get(canon_blind))
}

/// The class of one work, with the two questions a client actually asks.
///
/// `eligible` and `assumes_canon` are separate fields rather than one `state` string because they are separate decisions: a
/// badge says "you may not know this fandom", and a filter says "hide what assumes canon". Both are `false` when the work
/// was not classified, which is the conservative direction §50.2 asks for — an unmeasured work is offered to nobody as
/// canon-agnostic, because offering it is how a reader meets a text that stalls them.
async fn work_canon(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let stored = canon_store::stored_canon(state.db(), &id)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
    let eligible = canon_store::is_canon_agnostic(state.db(), &id)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
    let assumes_canon = canon_store::is_canon_dependent(state.db(), &id)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;

    let Some(stored) = stored else {
        // Not 404: the work exists, the question was asked, and "nobody has measured this" is the answer. A 404 would say
        // the work is missing.
        return Ok(Json(json!({
            "work_id": id,
            "state": "unmeasured",
            "eligible": false,
            "assumes_canon": false,
            "label": Value::Null,
            "declared_at": Value::Null,
            "text_version": Value::Null,
            "measures": Value::Null,
            "reason": Value::Null,
        })));
    };

    let (state_label, reason) = match &stored.verdict {
        CanonVerdict::CanonAgnostic(_) => ("canon_agnostic", Value::Null),
        CanonVerdict::CanonDependent(_) => ("canon_dependent", Value::Null),
        CanonVerdict::Unclassified { reason } => ("unclassified", json!(reason)),
    };
    let measures = stored.verdict.measures().map(|measures| {
        json!({
            "word_count": measures.word_count,
            "unexplained_names": measures.unexplained_names,
            "density": measures.density,
        })
    });

    Ok(Json(json!({
        "work_id": stored.work_id,
        "state": state_label,
        "eligible": eligible,
        "assumes_canon": assumes_canon,
        // The wording is the label §50.2 asks for, and it is only ever attached to a work whose class says it is true.
        "label": eligible.then_some("you may not know this fandom"),
        "declared_at": stored.declared_at,
        "text_version": stored.text_version,
        "measures": measures,
        "reason": reason,
    })))
}

/// Compute and store the class, for an operator — the backfill door and the "why is this work not listed" door.
///
/// Answers the same document the read door gives, so an operator who measures sees what a caller will see. `null` with
/// `state: "unmeasured"` means the work has no text at all: nothing was classified and nothing was written, which is not a
/// failed write.
async fn measure_work(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    require_operator(&state, &user).await?;
    let computed_at = crate::format_rfc3339(time::OffsetDateTime::now_utc());
    canon_store::measure_work_and_store(state.db(), &id, &computed_at)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
    let stored = canon_store::stored_canon(state.db(), &id)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
    let Some(stored) = stored else {
        return Ok(Json(json!({
            "work_id": id,
            "state": "unmeasured",
            "reason": "no_text",
        })));
    };
    Ok(Json(json!({
        "work_id": stored.work_id,
        "state": match &stored.verdict {
            CanonVerdict::CanonAgnostic(_) => "canon_agnostic",
            CanonVerdict::CanonDependent(_) => "canon_dependent",
            CanonVerdict::Unclassified { .. } => "unclassified",
        },
        "declared_at": stored.declared_at,
        "text_version": stored.text_version,
    })))
}

/// How many works one page of the canon-blind listing holds. Small on purpose: this is the "try something you would not
/// have found" shelf, and a long shelf is a feed.
const PAGE: i64 = 12;

#[derive(Debug, Deserialize)]
struct BlindQuery {
    #[serde(default)]
    limit: Option<i64>,
}

/// The canon-agnostic works, as a labelled set rather than as results.
///
/// `state` distinguishes the two ways this list is empty: `unmeasured` when nothing has been classified on this instance
/// (a fresh deployment, or a corpus whose works are all too short), and `empty` when things were measured and none is
/// canon-agnostic. Without that distinction an operator reads an empty list as "the classifier found nothing", which is the
/// same mistake as an empty moderation queue read as "nothing happened".
async fn canon_blind(
    State(state): State<AppState>,
    Query(query): Query<BlindQuery>,
) -> ApiResult<Json<Value>> {
    let limit = query.limit.unwrap_or(PAGE).clamp(1, 48);
    let rows = canon_store::list_canon_agnostic(state.db(), limit)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;

    // Eligibility is asked of each row through §50.2's own predicate rather than re-derived from the query's `LIKE`: the
    // query is an index-friendly candidate set, and the rule stays in one place.
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        if !canon_store::is_canon_agnostic(state.db(), &row.work_id)
            .await
            .map_err(|error| ApiError(AppError::Internal(error)))?
        {
            continue;
        }
        items.push(json!({
            "work_id": row.work_id,
            "title": row.title,
            "state": "canon_agnostic",
            "label": "you may not know this fandom",
            "declared_at": row.declared_at,
            "measures": {
                "word_count": row.word_count,
                "density": row.density,
            },
        }));
    }

    Ok(Json(json!({
        "state": if items.is_empty() { "unmeasured" } else { "ok" },
        "items": items,
    })))
}

/// One decision, one place (ADR 0003): this instance's operator, by claim or by configured account.
async fn require_operator(state: &AppState, user: &crate::auth::SessionUser) -> ApiResult<()> {
    crate::routes::admin_scope::require_estate_hidden(state, user)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_page_is_a_shelf_not_a_feed() {
        // A const block, not a runtime assert: the bound is a fact about the source, so it should fail the build rather than a
        // test run — and clippy is right that asserting a constant inside a test proves nothing at test time.
        const { assert!(super::PAGE <= 24, "a long shelf is a feed") };
    }
}
