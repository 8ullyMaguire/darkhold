//! ADR 0016 — where this instance's taste comes from, as a door.
//!
//! ```text
//! GET /taste    the folded profile, one report per declared source, and the declaration that produced it
//! ```
//!
//! # Why this door exists at all
//!
//! The estate's rule is that a capability which is declared `on` must publish the route it promises, and the gate calls it.
//! A site whose `taste` capability says `polaris-taste-source` while nothing can be asked about the profile is a
//! declaration with no witness — the exact defect the health programme kept finding, one layer up.
//!
//! # Why it is operator-only
//!
//! The profile names *who* set it (`sources` on every entry, `admitted` per source). That is the instance's own view of
//! who its trusted contributors are, and publishing it would turn a declaration into a leaderboard. The reader's own
//! dial and their own place in the corpus are already answered elsewhere (`/me/affinity`, `/me/badges`).

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::http::{ApiError, ApiResult};
use crate::state::AppState;
use darkhold_domain::AppError;
use polaris_taste_source::Evidence;

/// How many evidence rows one request folds. A first run on a large instance is bounded rather than a slow request; the
/// fold is linear, so the number is a patience budget, not a correctness one.
const EVIDENCE_LIMIT: i64 = 20_000;

/// The taste routes.
pub fn router() -> Router<AppState> {
    Router::new().route("/taste", get(profile))
}

/// The instance's taste profile.
async fn profile(
    State(state): State<AppState>,
    crate::auth::RequireSession(user): crate::auth::RequireSession,
) -> ApiResult<Json<Value>> {
    crate::routes::experiment_sweep::require_operator(&state, &user)?;

    let declaration = crate::taste_source::declaration(&state.config().taste)
        // A declaration the operator edited by hand is a validation failure, not a 500: the message names the source and
        // the vocabulary, and it is the only thing that makes the refusal actionable.
        .map_err(|message| ApiError(AppError::field("taste.source", message)))?;

    let operator = state
        .config()
        .administration
        .operator_account_id
        .map(|id| id.to_string());

    let contributor_rows = darkhold_db::taste_source::contributors(state.db())
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?;
    let evidence_rows = darkhold_db::taste_source::evidence(state.db(), EVIDENCE_LIMIT)
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?;

    let contributors = crate::taste_source::contributors(&contributor_rows, operator.as_deref());
    let (owned, unmapped) = crate::taste_source::evidence(&evidence_rows, &crate::taste_source::kind_map());
    let borrowed: Vec<Evidence<'_>> = owned.iter().map(crate::taste_source::OwnedEvidence::as_row).collect();

    let (profile, reports) = polaris_taste_source::fold(&declaration, &contributors, &borrowed);

    Ok(Json(json!({
        "declared": {
            "component": "polaris-taste-source",
            "half_life_days": declaration.half_life_days,
            "min_evidence": declaration.min_evidence,
            "sources": declaration.sources.iter().map(|source| json!({
                "name": source.name,
                "weight": source.weight,
                "kinds": source.kinds.iter().map(|kind| kind.name()).collect::<Vec<&str>>(),
            })).collect::<Vec<Value>>(),
        },
        "profile": profile.iter().map(|entry| json!({
            "subject": entry.subject,
            "weight": entry.weight,
            "contributions": entry.contributions,
            "sources": entry.sources,
        })).collect::<Vec<Value>>(),
        "sources": reports.iter().map(|report| json!({
            "name": report.name,
            "admitted": report.admitted,
            "contributed": report.contributed,
            "note": report.note,
        })).collect::<Vec<Value>>(),
        "measured": {
            "accounts": contributor_rows.len(),
            "evidence_rows": evidence_rows.len(),
            "unmapped_verbs": unmapped,
        },
        // An empty profile is an answer. The note says which answer it is, so nobody reads `[]` as a failure.
        "empty_reason": if profile.is_empty() {
            Some(reports.iter().map(|report| report.note.clone()).collect::<Vec<String>>().join("; "))
        } else {
            None
        },
    })))
}
