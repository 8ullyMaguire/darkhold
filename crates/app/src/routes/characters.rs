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
use axum::routing::{get, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use darkhold_domain::AppError;

use crate::auth::{MaybeSession, RequireSession};
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/works/{id}/characters", get(characters))
        // The authoring half. Every one of these is the work's owner's, not the operator's: the cast of a work is a claim
        // about the work, and nobody else can make it (M70-13).
        .route(
            "/works/{id}/characters/{node}",
            put(put_character).delete(delete_character),
        )
        .route(
            "/works/{id}/characters/{node}/roles/{role}",
            put(put_role).delete(delete_role),
        )
        .route("/works/{id}/characters/{node}/roles", get(list_roles))
        .route(
            "/works/{id}/relationships/{node}/dynamics",
            get(list_dynamics),
        )
        .route(
            "/works/{id}/characters/{node}/attributes/{attribute}",
            put(put_attribute).delete(delete_attribute),
        )
        .route("/works/{id}/relationships/{node}", put(put_relationship))
        .route(
            "/works/{id}/relationships/{node}/dynamics/{dynamic}",
            put(put_dynamic).delete(delete_dynamic),
        )
        .route(
            "/works/{id}/characters/{node}/relationships",
            get(relationships_involving),
        )
}

/// The work's owner, or the instance's answer to a stranger.
///
/// Ownership is the work's (`works.owner_pseud_id` → the caller's account), which is the same rule the tag doors use: a
/// claim about a work belongs to whoever wrote it, and an operator who could author one could put words in an author's work.
async fn require_owner(state: &AppState, work_id: &str, account: &str) -> ApiResult<()> {
    let owned = darkhold_db::orphan::owned_by(state.db(), work_id, account)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
    if !owned {
        // `404`, not `403`: whether this work is yours is already public, but whether *this account* is the author of a work
        // it may not be allowed to see is not something a refusal should confirm (ADR 0030).
        tracing::debug!(work = %work_id, account, "a non-owner tried to author a work's cast");
        return Err(ApiError(AppError::NotFound { resource: "work" }));
    }
    Ok(())
}

fn now() -> String {
    crate::format_rfc3339(time::OffsetDateTime::now_utc())
}

#[derive(Debug, Deserialize)]
struct CharacterBody {
    prominence: String,
    #[serde(default)]
    is_pov: bool,
}

/// Claim a character in this work, or change the claim about one already there.
async fn put_character(
    State(state): State<AppState>,
    session: RequireSession,
    Path((work_id, node)): Path<(String, String)>,
    Json(body): Json<CharacterBody>,
) -> ApiResult<Json<Value>> {
    let account = session.0.account_id.to_string();
    require_owner(&state, &work_id, &account).await?;
    if !darkhold_db::work_characters::PROMINENCE_VALUES.contains(&body.prominence.as_str()) {
        return Err(ApiError(AppError::field(
            "prominence",
            "one of protagonist · supporting · cameo · mentioned",
        )));
    }
    let character = darkhold_db::work_characters::WorkCharacter {
        work_id: work_id.clone(),
        character_node_id: node.clone(),
        prominence: body.prominence.clone(),
        is_pov: body.is_pov,
        added_at: now(),
        roles: Vec::new(),
    };
    // `created` is asked for before the write, not derived from it: the store's upsert reports whether it *wrote*, and an
    // update writes too — a rows-affected count cannot tell a first claim from a change to one, and a client that cannot tell
    // them apart cannot show "added" rather than "updated".
    let existed = darkhold_db::work_characters::character_in_work(state.db(), &work_id, &node)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?
        .is_some();
    darkhold_db::work_characters::upsert_character(state.db(), &character)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
    let created = !existed;
    // Read back: the row is the fact, and `created` only says whether this call is what wrote it.
    let stored = darkhold_db::work_characters::character_in_work(state.db(), &work_id, &node)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
    Ok(Json(json!({
        "created": created,
        "character": stored.map(|c| json!({
            "character_node_id": c.character_node_id,
            "prominence": c.prominence,
            "is_pov": c.is_pov,
            "roles": c.roles,
            "added_at": c.added_at,
        })),
    })))
}

/// Withdraw the claim about a character in this work.
///
/// `404` when there was no claim: "I withdrew it" and "there was nothing to withdraw" are different answers, and only one of
/// them is checkable.
async fn delete_character(
    State(state): State<AppState>,
    session: RequireSession,
    Path((work_id, node)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let account = session.0.account_id.to_string();
    require_owner(&state, &work_id, &account).await?;
    let removed = darkhold_db::work_characters::remove_character(state.db(), &work_id, &node)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
    if !removed {
        return Err(ApiError(AppError::NotFound {
            resource: "character in this work",
        }));
    }
    Ok(Json(json!({ "removed": true })))
}

async fn put_role(
    State(state): State<AppState>,
    session: RequireSession,
    Path((work_id, node, role)): Path<(String, String, String)>,
) -> ApiResult<Json<Value>> {
    let account = session.0.account_id.to_string();
    require_owner(&state, &work_id, &account).await?;
    let added = darkhold_db::work_characters::add_character_role(
        state.db(),
        &work_id,
        &node,
        &role,
        &now(),
    )
    .await
    .map_err(|error| ApiError(AppError::Internal(error)))?;
    Ok(Json(json!({ "added": added })))
}

async fn delete_role(
    State(state): State<AppState>,
    session: RequireSession,
    Path((work_id, node, role)): Path<(String, String, String)>,
) -> ApiResult<Json<Value>> {
    let account = session.0.account_id.to_string();
    require_owner(&state, &work_id, &account).await?;
    let removed =
        darkhold_db::work_characters::remove_character_role(state.db(), &work_id, &node, &role)
            .await
            .map_err(|error| ApiError(AppError::Internal(error)))?;
    if !removed {
        return Err(ApiError(AppError::NotFound {
            resource: "role on this character",
        }));
    }
    Ok(Json(json!({ "removed": true })))
}

async fn put_attribute(
    State(state): State<AppState>,
    session: RequireSession,
    Path((work_id, node, attribute)): Path<(String, String, String)>,
) -> ApiResult<Json<Value>> {
    let account = session.0.account_id.to_string();
    require_owner(&state, &work_id, &account).await?;
    let added = darkhold_db::work_characters::add_character_attribute(
        state.db(),
        &work_id,
        &node,
        &attribute,
        &now(),
    )
    .await
    .map_err(|error| ApiError(AppError::Internal(error)))?;
    Ok(Json(json!({ "added": added })))
}

async fn delete_attribute(
    State(state): State<AppState>,
    session: RequireSession,
    Path((work_id, node, attribute)): Path<(String, String, String)>,
) -> ApiResult<Json<Value>> {
    let account = session.0.account_id.to_string();
    require_owner(&state, &work_id, &account).await?;
    let removed = darkhold_db::work_characters::remove_character_attribute(
        state.db(),
        &work_id,
        &node,
        &attribute,
    )
    .await
    .map_err(|error| ApiError(AppError::Internal(error)))?;
    if !removed {
        return Err(ApiError(AppError::NotFound {
            resource: "attribute on this character",
        }));
    }
    Ok(Json(json!({ "removed": true })))
}

#[derive(Debug, Deserialize)]
struct RelationshipBody {
    rel_type: String,
    prominence: String,
    #[serde(default)]
    label: Option<String>,
    /// The pairing's ends. Sent as a whole set: the door replaces what is there rather than adding to it, so a client that
    /// retries gets the same pairing instead of a pairing with a character in it twice. (`ON CONFLICT DO NOTHING` would make
    /// that harmless rather than wrong, but "the set I sent is the set" is the easier promise to keep.)
    #[serde(default)]
    participants: Vec<String>,
    #[serde(default)]
    dynamics: Vec<String>,
}

/// Claim a relationship in this work: the pairing, its ends and its dynamics.
///
/// One door for the three because the store's three writes are one sentence — "these people are like this in this work" — and
/// a client that could send them separately could leave a pairing with one end.
async fn put_relationship(
    State(state): State<AppState>,
    session: RequireSession,
    Path((work_id, node)): Path<(String, String)>,
    Json(body): Json<RelationshipBody>,
) -> ApiResult<Json<Value>> {
    let account = session.0.account_id.to_string();
    require_owner(&state, &work_id, &account).await?;
    if !darkhold_db::work_characters::REL_TYPE_VALUES.contains(&body.rel_type.as_str()) {
        return Err(ApiError(AppError::field(
            "rel_type",
            "one of romantic · platonic · familial · qpp · sexual · antagonistic · other",
        )));
    }
    // The *pairing's* vocabulary, not the character's: `supporting` is a character word, and the schema's CHECK would turn
    // that mistake into a 500 after the request passed the door.
    if !darkhold_db::work_characters::REL_PROMINENCE_VALUES.contains(&body.prominence.as_str()) {
        return Err(ApiError(AppError::field(
            "prominence",
            "one of primary · secondary · background",
        )));
    }
    if body.participants.len() < 2 {
        return Err(ApiError(AppError::field(
            "participants",
            "a pairing has two ends",
        )));
    }

    let relationship = darkhold_db::work_characters::WorkRelationship {
        // The row id is generated here because the store's upsert keys on `(work_id, relationship_node_id, rel_type)` and
        // ignores this field on conflict — the id of the existing row is what stays.
        id: uuid::Uuid::new_v4().to_string(),
        work_id: work_id.clone(),
        relationship_node_id: node.clone(),
        rel_type: body.rel_type.clone(),
        prominence: body.prominence.clone(),
        dynamics: body.dynamics.clone(),
        label: body.label.clone(),
        added_at: now(),
    };
    // Same shape as `put_character`: the upsert reports that it wrote, which an update also does, so "created" is a question
    // asked before the write.
    let existed = darkhold_db::work_characters::relationships_for_work(state.db(), &work_id)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?
        .iter()
        .any(|r| r.relationship_node_id == node && r.rel_type == body.rel_type);
    darkhold_db::work_characters::upsert_relationship(state.db(), &relationship)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
    let created = !existed;

    // The participants are keyed by the pairing's **node**, not by the row id — the mistake the read door's first test
    // caught, and the reason this is spelled out.
    let existing = darkhold_db::work_characters::relationship_participants(state.db(), &node)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
    for participant in &body.participants {
        darkhold_db::work_characters::add_relationship_participant(state.db(), &node, participant)
            .await
            .map_err(|error| ApiError(AppError::Internal(error)))?;
    }
    for stale in existing {
        if !body.participants.contains(&stale) {
            darkhold_db::work_characters::remove_relationship_participant(
                state.db(),
                &node,
                &stale,
            )
            .await
            .map_err(|error| ApiError(AppError::Internal(error)))?;
        }
    }

    // The dynamics are keyed by the claim's row id (`work_relationship_dynamics.relationship_id` references
    // `work_relationships.id`), so this needs the row that is actually stored rather than the one just constructed.
    let stored = darkhold_db::work_characters::relationships_for_work(state.db(), &work_id)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?
        .into_iter()
        .find(|r| r.relationship_node_id == node && r.rel_type == body.rel_type);
    if let Some(stored) = stored {
        darkhold_db::work_characters::set_relationship_dynamics(
            state.db(),
            &stored.id,
            &body.dynamics,
            &now(),
        )
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
    }

    Ok(Json(json!({
        "created": created,
        "relationship_node_id": node,
        "rel_type": body.rel_type,
        "participants": body.participants,
        "dynamics": body.dynamics,
    })))
}

/// The roles one character holds in this work.
///
/// The cast door carries roles too, and this exists because the write doors are per role: a client that just added one wants
/// the answer to "what does the character have now", not the whole cast re-fetched.
async fn list_roles(
    State(state): State<AppState>,
    MaybeSession(_viewer): MaybeSession,
    Path((work_id, node)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let roles = darkhold_db::work_characters::roles_of_character(state.db(), &work_id, &node)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
    Ok(Json(json!({ "character_node_id": node, "roles": roles })))
}

/// The dynamics of one pairing.
///
/// Keyed by the claim (`(work, pairing node, rel_type)`'s row), because a work may claim `romantic` and `platonic` about the
/// same two people and each has its own dynamics.
async fn list_dynamics(
    State(state): State<AppState>,
    MaybeSession(_viewer): MaybeSession,
    Path((work_id, node)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let claim = claim_row(&state, &work_id, &node).await?;
    let dynamics = darkhold_db::work_characters::dynamics_of_relationship(state.db(), &claim)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
    Ok(Json(json!({
        "relationship_node_id": node,
        "relationship_id": claim,
        "dynamics": dynamics,
    })))
}

/// Add one dynamic to a pairing, without touching the others the pairing already has.
///
/// The bulk door (`PUT /works/{id}/relationships/{node}`) replaces the whole set, which is the honest shape for a form. This
/// one exists for the case that shape is wrong for: a client appending one observation, where "replace" would become "delete
/// everything I did not send".
async fn put_dynamic(
    State(state): State<AppState>,
    session: RequireSession,
    Path((work_id, node, dynamic)): Path<(String, String, String)>,
) -> ApiResult<Json<Value>> {
    let account = session.0.account_id.to_string();
    require_owner(&state, &work_id, &account).await?;
    let claim = claim_row(&state, &work_id, &node).await?;
    let added = darkhold_db::work_characters::add_relationship_dynamic(
        state.db(),
        &claim,
        &dynamic,
        &now(),
    )
    .await
    .map_err(|error| ApiError(AppError::Internal(error)))?;
    Ok(Json(json!({ "added": added })))
}

async fn delete_dynamic(
    State(state): State<AppState>,
    session: RequireSession,
    Path((work_id, node, dynamic)): Path<(String, String, String)>,
) -> ApiResult<Json<Value>> {
    let account = session.0.account_id.to_string();
    require_owner(&state, &work_id, &account).await?;
    let claim = claim_row(&state, &work_id, &node).await?;
    let removed =
        darkhold_db::work_characters::remove_relationship_dynamic(state.db(), &claim, &dynamic)
            .await
            .map_err(|error| ApiError(AppError::Internal(error)))?;
    if !removed {
        return Err(ApiError(AppError::NotFound {
            resource: "dynamic on this relationship",
        }));
    }
    Ok(Json(json!({ "removed": true })))
}

/// The stored `work_relationships.id` for a pairing in this work.
///
/// Dynamics are keyed by the *claim's* row id and participants by the pairing's *node* — two different strings for one
/// relationship, which is why a helper rather than a second inline lookup.
async fn claim_row(state: &AppState, work_id: &str, node: &str) -> ApiResult<String> {
    let found = darkhold_db::work_characters::relationships_for_work(state.db(), work_id)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?
        .into_iter()
        .find(|r| r.relationship_node_id == node);
    found.map(|r| r.id).ok_or(ApiError(AppError::NotFound {
        resource: "relationship in this work",
    }))
}

/// The relationships of this work that include one character.
async fn relationships_involving(
    State(state): State<AppState>,
    MaybeSession(_viewer): MaybeSession,
    Path((work_id, node)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let relationships =
        darkhold_db::work_characters::relationships_involving(state.db(), &work_id, &node)
            .await
            .map_err(|error| ApiError(AppError::Internal(error)))?;
    Ok(Json(json!({
        "character_node_id": node,
        "relationships": relationships
            .into_iter()
            .map(|r| json!({
                "id": r.id,
                "relationship_node_id": r.relationship_node_id,
                "rel_type": r.rel_type,
                "prominence": r.prominence,
                "label": r.label,
                "dynamics": r.dynamics,
            }))
            .collect::<Vec<Value>>(),
    })))
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

    let mut cast = Vec::with_capacity(characters.len());
    for character in characters {
        // Attributes are a second table keyed by the same triple. Folded in for the same reason as roles and participants:
        // a reader that has to ask again per character will not ask, and a cast list with half its detail missing reads as
        // "this work has no attributes" rather than "the page did not fetch them".
        let attributes = darkhold_db::work_characters::attributes_of_character(
            state.db(),
            &work_id,
            &character.character_node_id,
        )
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
        cast.push(json!({
            "character_node_id": character.character_node_id,
            "prominence": character.prominence,
            "is_pov": character.is_pov,
            "roles": character.roles,
            "attributes": attributes,
            "added_at": character.added_at,
        }));
    }

    Ok(Json(json!({
        "work_id": work_id,
        "characters": cast,
        "relationships": rows,
    })))
}
