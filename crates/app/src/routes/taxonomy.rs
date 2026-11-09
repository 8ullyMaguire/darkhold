//! Taxonomy routes: autocomplete, tagging, alias management.
//!
//! Spec §15.1–15.3.

use crate::auth::MaybeSession;
use crate::auth::{RequirePseud, RequireSession};
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use darkhold_db::taxonomy::{self, TagAssertion, TagRole, TagSource};
use darkhold_db::tag_confirmation::TagConfirmation;
use darkhold_db::{collaboration, media_resilience};
use darkhold_domain::WorkId;
use serde::Deserialize;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/taxonomy", get(autocomplete).post(create_taxonomy_node))
        .route("/taxonomy/{id}", get(get_node))
        .route("/taxonomy/aliases", post(create_taxonomy_alias))
        // M45-68: what `trope:<slug>` means, published — a group that expands in private is a
        // vocabulary nobody can learn.
        .route("/tropes", get(list_tropes))
        .route("/works/{id}/tags", post(tag_work))
        // The entity tree's only write. `entity_closure::rebuild` walks `canonical_entities.parent_id`, and nothing in the product
        // ever set it — so the closure was empty on every deployment and `+children` expansion (§11.4) could not expand anything.
        .route("/taxonomy/{id}/parent", put(set_entity_parent))
        // §6.4's reader layer: the only thing that moves a tag's confidence. Without this door the column was maintained by a sweep
        // nothing called, fed by a table nothing wrote, read by no query.
        .route(
            "/works/{id}/tags/{node}/vote",
            get(my_tag_vote).put(cast_tag_vote),
        )
        // §49.2's confirmation state, which decides whether a tag moves a reader's ranking at all. The store function had no caller
        // and every row on the deployed instance is `unconfirmed`, so the gravity filter was reading an empty set.
        .route(
            "/works/{id}/tags/{node}/confirmation",
            put(set_tag_confirmation),
        )
        // The repair half of §6.4. A vote recomputes its own row, so this door exists for the window the vote path leaves open —
        // a crash between the vote row and the aggregate — and for a restore that replayed votes without their confidence.
        .route(
            "/admin/taxonomy/confidence/recompute",
            post(recompute_confidence),
        )
}

/// `PUT /taxonomy/{id}/parent` — the write the entity tree never had.
///
/// `entity_closure::rebuild` walks `canonical_entities.parent_id`, and nothing in the product ever set it: the deployed instance
/// has 0 of 58,042 entities with a parent, 0 closure rows and 0 edges, so §11.4's `+children` expansion could not expand anything
/// on any deployment. A closure rebuild with nothing to rebuild is a sweep that has always been right about an empty tree.
///
/// **A curator act, and a published one.** The refusal to a non-curator is `403` rather than `404`: a curator door a curator
/// cannot find is a door that does not work, and `is_active_curator` is the same predicate the tag door above uses.
///
/// **`null` detaches.** A curator who attached a node to the wrong parent must be able to say so, and an empty string is a
/// malformed id rather than an absence — the two are different requests.
///
/// **The closure follows on the maintenance pass (~30s), not here.** A rebuild is one INSERT per entity; a curator act must not
/// cost that. The response says `closure: "pending_maintenance"` so a caller is told rather than left to assume.
#[derive(Debug, Deserialize)]
pub struct ParentBody {
    #[serde(default)]
    pub parent_id: Option<String>,
}

async fn set_entity_parent(
    State(state): State<AppState>,
    RequirePseud { user, .. }: RequirePseud,
    Path(id): Path<String>,
    Json(body): Json<ParentBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let account_id = user.account_id.to_string();
    let is_curator = media_resilience::is_active_curator(state.db(), &account_id)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;
    if !is_curator {
        return Err(ApiError(darkhold_domain::AppError::AccessDenied));
    }
    // An empty id is a malformed id, not an absence: `null` is how a caller says "no parent", and accepting `""` would make a
    // typo look like a deliberate detach.
    if matches!(body.parent_id.as_deref(), Some("")) {
        return Err(ApiError(darkhold_domain::AppError::field(
            "parent_id",
            "an empty id is not an absence; send `null` to detach the node",
        )));
    }

    match darkhold_db::entity_closure::set_parent(
        state.db(),
        &id,
        body.parent_id.as_deref(),
        &account_id,
    )
    .await
    {
        Ok(()) => Ok(Json(serde_json::json!({
            "node_id": id,
            "parent_id": body.parent_id,
            "closure": "pending_maintenance",
        }))),
        Err(darkhold_db::entity_closure::ParentError::Unknown(which)) => {
            Err(ApiError(darkhold_domain::AppError::NotFound { resource: which }))
        }
        Err(darkhold_db::entity_closure::ParentError::NotBrowsable(which)) => Err(ApiError(
            darkhold_domain::AppError::field(
                "parent_id",
                format!("the {which} is below the floor, so it is not a node in the tree"),
            ),
        )),
        Err(darkhold_db::entity_closure::ParentError::SelfParent) => Err(ApiError(
            darkhold_domain::AppError::field("parent_id", "a node cannot be its own parent"),
        )),
        Err(darkhold_db::entity_closure::ParentError::Cycle) => Err(ApiError(
            darkhold_domain::AppError::field("parent_id", "that edge would close a cycle"),
        )),
        Err(darkhold_db::entity_closure::ParentError::Db(error)) => {
            Err(ApiError(darkhold_domain::AppError::Internal(error.into())))
        }
    }
}

/// `PUT /works/{id}/tags/{node}/confirmation` — what the gravity filter was waiting for.
///
/// `tag_confirmation::confirm` writes the state `gravity_contributing_tags` reads, and had no caller: every row on the deployed
/// instance is `unconfirmed`, so §49.2's filter returned nothing for every work and the gravity layer was reading an empty tag
/// set on every surface.
///
/// **A reader confirms or flags; only a curator may claim `wrangler`.** §49.2 gives the reader two acts and the wrangler a third,
/// and a state is a claim about authority — a session that could set `wrangler` would launder a scoped opinion into a curation.
/// The check is the same `is_active_curator` the tag door above uses, so the two agree about who a curator is.
///
/// **`unconfirmed` is refused.** It is the absence of a judgement, which is what every imported row already says; accepting it
/// would let a request erase somebody's confirmation, and erasing recorded speech is a different act with its own audit trail.
///
/// **A tag the work does not carry is `404`.** There is no claim to judge, and `422` would blame a well-formed request — the same
/// decision the vote door beside it made.
#[derive(Debug, Deserialize)]
pub struct ConfirmationBody {
    pub state: String,
}

async fn set_tag_confirmation(
    State(state): State<AppState>,
    RequirePseud { user, pseud_id }: RequirePseud,
    Path((id, node)): Path<(String, String)>,
    Json(body): Json<ConfirmationBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let work_id: WorkId = id
        .parse()
        .map_err(|_| ApiError(darkhold_domain::AppError::NotFound { resource: "work" }))?;

    let confirmation = match body.state.as_str() {
        "reader" => TagConfirmation::Reader,
        "inaccurate" => TagConfirmation::Inaccurate,
        "wrangler" => {
            let account_id = user.account_id.to_string();
            let is_curator = media_resilience::is_active_curator(state.db(), &account_id)
                .await
                .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;
            if !is_curator {
                return Err(ApiError(darkhold_domain::AppError::AccessDenied));
            }
            TagConfirmation::Wrangler
        }
        other => {
            return Err(ApiError(darkhold_domain::AppError::field(
                "state",
                if other == "unconfirmed" {
                    "`unconfirmed` is the absence of a judgement, not one a caller can assert; confirm or flag the tag instead"
                } else {
                    "must be `reader`, `wrangler` (curators only) or `inaccurate`"
                },
            )));
        }
    };

    // The flag records the **pseud**: it is the identity that made the judgement, it is what the work's page can act on, and a
    // review queue that needs the account can resolve it from the pseud. An account id here would name a person for what a
    // persona said, which is the distinction 0110's column is for.
    let flagged_by = if confirmation == TagConfirmation::Inaccurate {
        Some(pseud_id.to_string())
    } else {
        None
    };
    let applied = darkhold_db::tag_confirmation::confirm(
        state.db(),
        &work_id.to_string(),
        &node,
        confirmation,
        flagged_by.as_deref(),
    )
    .await
    .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;
    if !applied {
        return Err(ApiError(darkhold_domain::AppError::NotFound { resource: "tag" }));
    }

    Ok(Json(serde_json::json!({
        "state": confirmation.as_str(),
        "counts_toward_gravity": confirmation.counts_toward_gravity(),
    })))
}

#[derive(Debug, Deserialize)]
pub struct AutocompleteQuery {
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    prefix: Option<String>,
    #[serde(default = "default_limit")]
    limit: i64,
    /// Ask for the canonical prefix match rather than the fuzzy one.
    ///
    /// Two different questions share this door and they cannot share an answer. Typing into a tag box wants fuzzy — "slow
    /// burn" should find "slow burn (tag)". The metadata exchange wants exact: it asks whether a canonical name is present
    /// before importing, and a fuzzy hit would silently bind a work to a tag it never claimed (M70-13).
    #[serde(default)]
    exact: bool,
}

fn default_limit() -> i64 {
    20
}

#[derive(Debug, Deserialize)]
pub struct CreateNodeBody {
    kind: String,
    canonical: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateAliasBody {
    alias: String,
    node_id: String,
}

#[derive(Debug, Deserialize)]
pub struct TagVoteBody {
    /// `1` — the tag is right; `-1` — it is wrong; `0` — withdraw the vote.
    vote: i64,
}

#[derive(Debug, Deserialize)]
pub struct TagWorkBody {
    node_id: String,
    /// The tag's prominence: how much the work leans on it.
    ///
    /// **Prominence and not weight**, because the number was always a
    /// prominence and was called the other thing (spec §6.4).
    #[serde(default)]
    prominence: Option<i64>,
    /// The old spelling, still accepted.
    #[serde(default)]
    weight: Option<i64>,
    /// The claim's role: `primary` · `secondary` · `background`.
    ///
    /// The caller may say how strongly the work leans on a tag; it may not
    /// say *where the claim came from*. That is the request's provenance, and
    /// provenance is a property of the code path that made the claim, not of
    /// the JSON it carried.
    #[serde(default)]
    role: Option<String>,
}

async fn autocomplete(
    State(state): State<AppState>,
    RequireSession(_): RequireSession,
    Query(params): Query<AutocompleteQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let kind = params.kind.as_deref();
    let prefix = params.prefix.as_deref().unwrap_or("");
    let nodes = if params.exact {
        taxonomy::search_nodes(state.db(), kind, prefix, params.limit)
            .await
            .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?
    } else {
        taxonomy::search_nodes_fuzzy(state.db(), kind, prefix, params.limit)
            .await
            .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?
    };
    Ok(Json(serde_json::json!({ "items": nodes })))
}

async fn get_node(
    State(state): State<AppState>,
    RequireSession(_): RequireSession,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let node = taxonomy::node_by_id(state.db(), &id)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;
    match node {
        Some(n) => Ok(Json(serde_json::json!({ "node": n }))),
        None => Err(ApiError(darkhold_domain::AppError::NotFound {
            resource: "node",
        })),
    }
}

async fn create_taxonomy_node(
    State(state): State<AppState>,
    RequireSession(_): RequireSession,
    Json(body): Json<CreateNodeBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let node = taxonomy::create_node(state.db(), &body.kind, &body.canonical)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;
    Ok(Json(serde_json::json!({ "node": node })))
}

async fn create_taxonomy_alias(
    State(state): State<AppState>,
    RequireSession(_): RequireSession,
    Json(body): Json<CreateAliasBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let alias = taxonomy::create_alias(state.db(), &body.alias, &body.node_id, "author")
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;
    Ok(Json(serde_json::json!({ "alias": alias })))
}

/// Tag a work, as one of its authors or as a curator.
///
/// **The provenance is derived, never accepted.** `source` says where a claim
/// came from, so letting the request name it would make the column a claim
/// about a claim: a session could post `{"source": "imported"}` and launder a
/// person's assertion into the importer's, or post `{"source": "curator"}` and
/// dress it in authority it does not have. The two code paths that may tag a
/// work here are the author's (this pseud is one of the work's contributors)
/// and a curator's (this account holds an active curator role); anything else
/// is refused, because the specification's fifth provenance value is not
/// "a signed-in reader" -- a reader's channel is a vote
/// (`work_tag_votes`), which feeds `confidence` and never gravity
/// (ADR 0026).
async fn tag_work(
    State(state): State<AppState>,
    RequirePseud { user, pseud_id }: RequirePseud,
    Path(id): Path<String>,
    Json(body): Json<TagWorkBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let work_id: WorkId = id
        .parse()
        .map_err(|_| ApiError(darkhold_domain::AppError::NotFound { resource: "work" }))?;

    let contributors = collaboration::contributors_for_work(state.db(), work_id)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;
    let source = if contributors.iter().any(|c| c.pseud_id == pseud_id) {
        TagSource::Author
    } else {
        let account_id = user.account_id.to_string();
        let is_curator = media_resilience::is_active_curator(state.db(), &account_id)
            .await
            .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;
        if !is_curator {
            // Not an author of this work and not a curator: there is no
            // provenance value that would describe the claim honestly, so
            // there is no claim.
            return Err(ApiError(darkhold_domain::AppError::AccessDenied));
        }
        TagSource::Curator
    };

    // The role is a strength, not a provenance, so the caller may choose it --
    // but the default is the modest one: a bare tag is not the work's headline
    // description until someone says it is. (The importer and the inheritance
    // path assert `primary` when they copy a source's own tags; that is the
    // source's claim being carried over, not this route's judgement.)
    let assertion = TagAssertion {
        prominence: body.prominence.or(body.weight).unwrap_or(0),
        source,
        role: body
            .role
            .as_deref()
            .map(TagRole::parse)
            .unwrap_or(TagRole::Secondary),
    };
    let tag = taxonomy::tag_work(state.db(), &work_id.to_string(), &body.node_id, assertion)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;
    Ok(Json(serde_json::json!({ "tag": tag })))
}

/// `GET /api/v1/tropes` — every trope group, its label and the tags it expands to (M45-68).
///
/// Public and static: the groups ship with the binary, so this is a document, not a query. A reader
/// (or a client) can see exactly what `trope:dark-progression` will match before running it.
async fn list_tropes(
    State(_state): State<AppState>,
    MaybeSession(_user): MaybeSession,
) -> Json<serde_json::Value> {
    let items: Vec<serde_json::Value> = darkhold_domain::tropes::TROPES
        .iter()
        .map(|group| {
            serde_json::json!({
                "slug": group.slug,
                "label": group.label,
                "summary": group.summary,
                "expands_to": group.members,
            })
        })
        .collect();
    Json(serde_json::json!({ "items": items }))
}

/// §6.4's vote: a reader saying a tag is right, wrong, or taking the claim back.
///
/// Three things this door refuses to do:
///
/// * **It does not accept anybody else's vote as context.** The caller's pseud is the vote's identity; a request may say *what* it
///   thinks and never *who* it is.
/// * **It does not publish how other readers voted.** The answer carries *your* vote and the tag's confidence, and nothing about
///   the people who made that number. A pseud-scoped opinion shown with its owner is a durable public judgement about somebody
///   else's work.
/// * **It does not treat a tag the work does not carry as a malformed request.** There is no claim to judge, so the answer is
///   `404`; `422` would blame the caller for a request that was well formed.
async fn cast_tag_vote(
    State(state): State<AppState>,
    RequirePseud { pseud_id, .. }: RequirePseud,
    Path((work, node)): Path<(String, String)>,
    Json(body): Json<TagVoteBody>,
) -> ApiResult<Json<serde_json::Value>> {
    if !(-1..=1).contains(&body.vote) {
        let message = "a vote is 1 (the tag is right), -1 (it is wrong) or 0 (withdraw)".to_owned();
        return Err(ApiError(darkhold_domain::AppError::Validation {
            field_errors: std::iter::once(("vote".to_owned(), message.clone())).collect(),
            message,
        }));
    }
    let confidence = taxonomy::cast_tag_vote(
        state.db(),
        &work,
        &node,
        &pseud_id.to_string(),
        body.vote as i8,
    )
    .await
    .map_err(|e| ApiError::from(anyhow::Error::new(e)))?;
    match confidence {
        Some(confidence) => Ok(Json(serde_json::json!({
            "vote": if body.vote == 0 { serde_json::Value::Null } else { serde_json::json!(body.vote) },
            "confidence": confidence,
        }))),
        None => Err(ApiError(darkhold_domain::AppError::NotFound {
            resource: "tag",
        })),
    }
}

/// What the caller has said about one tag, beside the confidence everybody shares.
async fn my_tag_vote(
    State(state): State<AppState>,
    RequirePseud { pseud_id, .. }: RequirePseud,
    Path((work, node)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let confidence = taxonomy::tag_confidence(state.db(), &work, &node)
        .await
        .map_err(|e| ApiError::from(anyhow::Error::new(e)))?;
    let Some(confidence) = confidence else {
        return Err(ApiError(darkhold_domain::AppError::NotFound {
            resource: "tag",
        }));
    };
    let vote = taxonomy::tag_vote_for(state.db(), &work, &node, &pseud_id.to_string())
        .await
        .map_err(|e| ApiError::from(anyhow::Error::new(e)))?;
    Ok(Json(serde_json::json!({
        "vote": vote,
        "confidence": confidence,
    })))
}
/// Recompute every tag's confidence from the votes, as an operator repair pass.
///
/// Not a scheduled job and not a pre-render step: the vote path already keeps its own row current, so running this on a timer would
/// be work whose only result is a number that is already right. It answers with how many tag rows it touched, because "0" and
/// "nothing happened" have to be distinguishable to the operator who just ran it.
async fn recompute_confidence(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<serde_json::Value>> {
    crate::routes::admin_scope::require_admin_of(&state, &user, None).await?;
    let updated = taxonomy::recompute_confidence(state.db())
        .await
        .map_err(|e| ApiError::from(anyhow::Error::new(e)))?;
    Ok(Json(serde_json::json!({ "updated": updated })))
}
