//! Who is in a work, and how they are related — the read surface the character store never had (M70-13).
//!
//! The store ([`darkhold_db::work_characters`]) could write characters, roles, relationships and relationship
//! participants, and nothing could read any of it over HTTP: `characters_for_work`, `relationships_for_work` and
//! `relationship_participants` had no caller at all, inside `crates/db` or outside it. A feature with a schema, a store and
//! tests, and no door.
//!
//! ## Why the participants are folded in
//!
//! A relationship row is a *pairing*: its identity is a `canonical_entities` row of kind `relationship`, and its two ends
//! live in `relationship_participants`. Returning the relationships without their participants would hand the caller a list
//! of ids that all look alike — the page would have to ask twice for each, which is the N+1 the store exists to avoid, and a
//! client that could not be bothered would render "a relationship" with nobody in it.

use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};

use darkhold_domain::AppError;

use crate::auth::MaybeSession;
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/works/{id}/characters", get(characters))
}

/// The characters a work claims, and the relationships it claims between them.
///
/// Public, and it has to be: a work's cast is part of the work's description, and §15.1's prominence is a claim the author
/// made about their own work rather than a statement about a reader. `MaybeSession` is the extractor because a signed-out
/// visitor reading a work should see the same cast a signed-in one does.
async fn characters(
    State(state): State<AppState>,
    MaybeSession(_viewer): MaybeSession,
    Path(work_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let characters = darkhold_db::work_characters::characters_for_work(state.db(), &work_id)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;

    let relationships = darkhold_db::work_characters::relationships_for_work(state.db(), &work_id)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;

    let mut rows = Vec::with_capacity(relationships.len());
    for relationship in relationships {
        // Keyed by the pairing's **node**, not by this row's id: `relationship_participants` stores
        // `relationship_node_id`, which is a `canonical_entities` row (§6.4), and a `work_relationships` row id looks
        // equally plausible and returns an empty list — the shape of mistake this test caught on its first run.
        let participants = darkhold_db::work_characters::relationship_participants(
            state.db(),
            &relationship.relationship_node_id,
        )
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
        rows.push(json!({
            "id": relationship.id,
            "relationship_node_id": relationship.relationship_node_id,
            "rel_type": relationship.rel_type,
            "prominence": relationship.prominence,
            "dynamics": relationship.dynamics,
            "label": relationship.label,
            "added_at": relationship.added_at,
            // The pairing's two ends, in the store's order. Named `participants` rather than `characters` because a
            // relationship's ends are character *ids*, and a page that wants names joins them against `characters` above.
            "participants": participants,
        }));
    }

    Ok(Json(json!({
        "work_id": work_id,
        "characters": characters
            .into_iter()
            .map(|character| json!({
                "character_node_id": character.character_node_id,
                "prominence": character.prominence,
                "is_pov": character.is_pov,
                "roles": character.roles,
                "added_at": character.added_at,
            }))
            .collect::<Vec<Value>>(),
        "relationships": rows,
    })))
}
