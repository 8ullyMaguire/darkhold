//! M45 — Roadmap consensus routes (spec §44).
//!
//! The board is public; voting and suggesting are TL>=1; stage moves are
//! operator-only.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::{MaybeSession, RequireSession};
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use darkhold_db::{governance, roadmap};
use darkhold_domain::consensus::{K_FACTOR, START_RATING};
use darkhold_domain::AppError;
use std::collections::BTreeMap;

pub fn read_router() -> Router<AppState> {
    Router::new()
        .route("/roadmap", get(get_board))
        .route("/roadmap/cards/{id}", get(get_card))
        .route("/roadmap/changelog", get(get_changelog))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/roadmap/arena", get(get_arena).post(post_arena_vote))
        .route("/roadmap/suggest", post(post_suggest))
}

pub fn admin_router() -> Router<AppState> {
    Router::new().route("/admin/roadmap/move", post(post_move_card))
}

/// `GET /api/v1/roadmap` — the full board, grouped by stage. Public (spec §44.5).
pub async fn get_board(
    _maybe: MaybeSession,
    State(state): State<AppState>,
) -> ApiResult<Json<Value>> {
    let cards = roadmap::list_cards(state.db(), None)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    let grouped = group_by_stage(cards);
    Ok(Json(json!({ "board": grouped })))
}

/// Group cards by stage, each bucket ordered by Elo DESC.
fn group_by_stage(cards: Vec<darkhold_db::roadmap::Card>) -> serde_json::Map<String, Value> {
    let mut map = serde_json::Map::new();
    for card in cards {
        let entry = map
            .entry(card.stage.clone())
            .or_insert_with(|| Value::Array(Vec::new()));
        if let Value::Array(arr) = entry {
            arr.push(json!({
                "id": card.id,
                "title": card.title,
                // §44.1: the board LISTS the title only, but the payload
                // carries the body so clicking a card costs no second request.
                // §44.5 records the resulting ~600 KB response as a deliberate
                // trade against a loading state on the detail view.
                "body": card.body,
                "category": card.category,
                "elo_rating": card.elo_rating,
                "matches_played": card.matches_played,
                "times_best": card.times_best,
                "times_worst": card.times_worst,
            }));
        }
    }
    map
}

/// `GET /api/v1/roadmap/cards/:id` — one card in full, including its body.
///
/// Public and session-free, on the same reasoning as the board: §44.5 makes
/// prioritization public, and a body is part of what is public about a card.
/// A member-gated body would make the arena's own reasoning less legible to
/// exactly the audience the arena exists for.
///
/// §3.3: an unknown id is a 404 naming a coarse noun, never an identifier, so
/// the response cannot be used to confirm that a given id exists.
pub async fn get_card(
    _maybe: MaybeSession,
    State(state): State<AppState>,
    Path(card_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let card = roadmap::find_card_by_id(state.db(), &card_id)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?
        .ok_or_else(|| ApiError(AppError::NotFound { resource: "card" }))?;
    Ok(Json(json!({ "card": {
        "id": card.id,
        "title": card.title,
        "body": card.body,
        "category": card.category,
        "stage": card.stage,
        "elo_rating": card.elo_rating,
        "matches_played": card.matches_played,
        "times_best": card.times_best,
        "times_worst": card.times_worst,
    }})))
}

/// `GET /api/v1/roadmap/arena` — one ballot (4 idea cards + pre-match Elo).
pub async fn get_arena(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<Value>> {
    let account_id = user.account_id.to_string();
    let trust = governance::trust_for(state.db(), &account_id)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    // §29.4's gate, and it is `state.config().roadmap.min_trust` rather than
    // a literal: the previous `1` made §29.4's sentence decorative, because an
    // operator could not raise the bar without a code change. The default is
    // still 1, so no existing instance changes who may ballots.
    //
    // `<` and not `<=`: a bar of 1 means trust 1 may ballot, which is what the
    // old literal did. `<=` would exclude exactly the readers this replaces.
    let min_trust = state.config().roadmap.min_trust;
    if trust < min_trust {
        return Err(ApiError(AppError::Validation {
            // The bar is in the message. "trust level too low" is true and
            // useless; a reader who is told what would be enough can act on it.
            message: format!(
                "trust level too low to get an arena ballot: this instance needs trust level \
                 {min_trust} and yours is {trust}"
            ),
            field_errors: BTreeMap::new(),
        }));
    }

    let candidates = roadmap::arena_candidates(state.db(), 4)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    if candidates.len() < 4 {
        return Ok(Json(
            json!({ "ballot": null, "reason": "not_enough_cards" }),
        ));
    }

    let ballot_id = uuid::Uuid::new_v4().to_string();
    let card_ids: Vec<String> = candidates.iter().map(|c| c.id.clone()).collect();
    let served_elo: Vec<(String, f64)> = candidates
        .iter()
        .map(|c| (c.id.clone(), c.elo_rating))
        .collect();

    roadmap::create_ballot(state.db(), &ballot_id, &card_ids, &served_elo, &account_id)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;

    // M69-10: under Glicko-2 the ballot tells the reader how settled each card is. `null` rather than a number when the
    // switch is off, and `null` per card when the card has never been rated under it — an absent deviation is "not
    // measured", never 0, for the same reason an absent volatility is not 0.06.
    let positions = if state.config().roadmap.glicko {
        Some(
            darkhold_db::roadmap_glicko::load_all(state.db())
                .await
                .map_err(|e| ApiError(AppError::Internal(e)))?,
        )
    } else {
        None
    };
    let cards: Vec<Value> = candidates
        .iter()
        .map(|c| {
            let mut card = json!({
                "id": c.id,
                "title": c.title,
                // §44.1: a MaxDiff choice is a judgement about the feature, so the
                // ballot carries the body. Four cards is not a payload problem, and
                // this is the surface where the judgement is actually made.
                "body": c.body,
                "category": c.category,
                "elo_rating": c.elo_rating,
            });
            if let Some(positions) = &positions {
                let stored = positions.get(&c.id).copied();
                let rating = darkhold_db::roadmap_glicko::position_for(c.elo_rating, stored);
                card["glicko"] = json!({
                    "rating": rating.rating,
                    "deviation": rating.deviation,
                    "volatility": rating.volatility,
                    "confidence": rating.confidence(),
                    "diverged": stored.map(|position| position.diverged).unwrap_or(false),
                });
            }
            card
        })
        .collect();

    Ok(Json(json!({
        "ballot_id": ballot_id,
        "cards": cards,
        "served_elo": served_elo,
        "ladder": if state.config().roadmap.glicko { "glicko2" } else { "elo" },
    })))
}

/// `POST /api/v1/roadmap/arena` — submit a best/worst vote.
#[derive(Debug, Deserialize)]
pub struct ArenaVoteBody {
    pub ballot_id: String,
    pub best_id: String,
    pub worst_id: String,
}

pub async fn post_arena_vote(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Json(body): Json<ArenaVoteBody>,
) -> ApiResult<Json<Value>> {
    let account_id = user.account_id.to_string();
    let trust = governance::trust_for(state.db(), &account_id)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    // §29.4's gate, and it is `state.config().roadmap.min_trust` rather than
    // a literal: the previous `1` made §29.4's sentence decorative, because an
    // operator could not raise the bar without a code change. The default is
    // still 1, so no existing instance changes who may votes.
    //
    // `<` and not `<=`: a bar of 1 means trust 1 may vote, which is what the
    // old literal did. `<=` would exclude exactly the readers this replaces.
    let min_trust = state.config().roadmap.min_trust;
    if trust < min_trust {
        return Err(ApiError(AppError::Validation {
            // The bar is in the message. "trust level too low" is true and
            // useless; a reader who is told what would be enough can act on it.
            message: format!(
                "trust level too low to vote: this instance needs trust level \
                 {min_trust} and yours is {trust}"
            ),
            field_errors: BTreeMap::new(),
        }));
    }

    if body.best_id == body.worst_id {
        return Err(ApiError(AppError::Validation {
            message: "best and worst must be distinct".into(),
            field_errors: BTreeMap::new(),
        }));
    }

    // One vote per ballot per account: atomic UPDATE … WHERE voted_at IS NULL.
    let voted = roadmap::mark_voted(state.db(), &body.ballot_id)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    if !voted {
        return Err(ApiError(AppError::Validation {
            message: "already voted on this ballot".into(),
            field_errors: BTreeMap::new(),
        }));
    }

    // Fetch served Elo and compute updates.
    let (card_ids, served_elo, _) = roadmap::fetch_ballot(state.db(), &body.ballot_id)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?
        .ok_or_else(|| ApiError(AppError::NotFound { resource: "ballot" }))?;

    let find_rating = |id: &str| {
        served_elo
            .iter()
            .find(|(cid, _)| cid == id)
            .map(|(_, r)| *r)
    };

    let best_rating = find_rating(&body.best_id).ok_or_else(|| {
        ApiError(AppError::Validation {
            message: "best_id not in this ballot".into(),
            field_errors: BTreeMap::new(),
        })
    })?;
    let worst_rating = find_rating(&body.worst_id).ok_or_else(|| {
        ApiError(AppError::Validation {
            message: "worst_id not in this ballot".into(),
            field_errors: BTreeMap::new(),
        })
    })?;

    // Collect unchosen card IDs and their ratings.
    let unchosen: Vec<(String, f64)> = card_ids
        .iter()
        .filter(|id| *id != &body.best_id && *id != &body.worst_id)
        .map(|id| {
            let r = find_rating(id).unwrap_or(START_RATING);
            (id.clone(), r)
        })
        .collect();

    // M69-10: when the instance ranks by Glicko-2, the ballot produces a rating **and** the uncertainty that goes with it.
    // The Elo column keeps receiving the same number so the board's existing ordering, and every reader of it, is unchanged:
    // `rating` is Glicko-2's position on the same 1500-mean scale, not a second ladder.
    let mut glicko_report: Vec<Value> = Vec::new();
    let updates: Vec<(String, f64, bool, bool)> = if state.config().roadmap.glicko {
        let stored = darkhold_db::roadmap_glicko::load_all(state.db())
            .await
            .map_err(|e| ApiError(AppError::Internal(e)))?;
        let positions: Vec<(String, darkhold_domain::glicko::Rating)> = card_ids
            .iter()
            .map(|id| {
                let elo = find_rating(id).unwrap_or(START_RATING);
                let position = stored.get(id).copied();
                (
                    id.clone(),
                    darkhold_db::roadmap_glicko::position_for(elo, position),
                )
            })
            .collect();
        let rated = darkhold_domain::glicko::rate_ballot(&positions, &body.best_id, &body.worst_id);
        let now = darkhold_db::identity::now_rfc3339();
        for (card_id, update) in &rated {
            darkhold_db::roadmap_glicko::save(
                state.db(),
                card_id,
                update.after,
                update.diverged,
                &now,
            )
            .await
            .map_err(|e| ApiError(AppError::Internal(e)))?;
            glicko_report.push(json!({
                "card_id": card_id,
                "rating": update.after.rating,
                "deviation": update.after.deviation,
                "volatility": update.after.volatility,
                "confidence": update.after.confidence(),
                "diverged": update.diverged,
            }));
        }
        rated
            .iter()
            .map(|(card_id, update)| {
                (
                    card_id.clone(),
                    update.after.rating,
                    card_id == &body.best_id,
                    card_id == &body.worst_id,
                )
            })
            .collect()
    } else {
        let unchosen_ratings: Vec<f64> = unchosen.iter().map(|(_, r)| *r).collect();
        let outcome = darkhold_domain::consensus::maxdiff_elo_updates(
            best_rating,
            worst_rating,
            &unchosen_ratings,
            K_FACTOR,
        );

        // Build updates: (card_id, new_elo, is_best, is_worst).
        let mut updates: Vec<(String, f64, bool, bool)> = Vec::new();
        updates.push((
            body.best_id.clone(),
            best_rating + outcome.best_delta,
            true,
            false,
        ));
        updates.push((
            body.worst_id.clone(),
            worst_rating + outcome.worst_delta,
            false,
            true,
        ));
        for (i, (id, orig_r)) in unchosen.iter().enumerate() {
            updates.push((
                id.clone(),
                orig_r + outcome.unchosen_deltas[i],
                false,
                false,
            ));
        }
        updates
    };

    roadmap::apply_elo_and_counters(state.db(), &updates)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;

    if state.config().roadmap.glicko {
        return Ok(Json(
            json!({ "status": "recorded", "ladder": "glicko2", "cards": glicko_report }),
        ));
    }
    Ok(Json(json!({ "status": "recorded", "ladder": "elo" })))
}

/// `POST /api/v1/roadmap/suggest` — suggest a feature (TL>=1).
#[derive(Debug, Deserialize)]
pub struct SuggestBody {
    pub title: String,
    /// §44.1: an optional description of what is being suggested.
    ///
    /// Note what this endpoint does NOT do, because the spec's phrasing
    /// ("otherwise creates an `idea` card") overstates the code: it records a
    /// row in `roadmap_suggestions` and never creates a card. Cards come from
    /// the seeder or from an operator. A body therefore has no card to live on
    /// here, and forcing one to be created would make every typo a permanent
    /// arena card — a spam vector on the one surface that is public.
    ///
    /// So the body is carried on the SUGGESTION, and an operator promoting a
    /// suggestion to a card copies it across. That keeps §44.1's rule that the
    /// description is the operator's, and it keeps the arena's membership under
    /// operator control rather than under whoever typed fastest.
    ///
    /// Absent is `None`, distinct from an empty string, because "no description
    /// offered" and "an empty description" are different submissions.
    pub body: Option<String>,
}

/// §44.1: the bound on a member-supplied card body.
///
/// 8,000 characters is roughly two pages, comfortably more than the operator's
/// CSV bodies and short enough that the field cannot be used as a data store.
/// The CSV is the authority for real bodies and sets no bound; this is the
/// member-facing edge only.
const MAX_SUGGESTED_BODY: usize = 8_000;

pub async fn post_suggest(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Json(body): Json<SuggestBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let account_id = user.account_id.to_string();
    let trust = governance::trust_for(state.db(), &account_id)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    // §29.4's gate, and it is `state.config().roadmap.min_trust` rather than
    // a literal: the previous `1` made §29.4's sentence decorative, because an
    // operator could not raise the bar without a code change. The default is
    // still 1, so no existing instance changes who may suggests.
    //
    // `<` and not `<=`: a bar of 1 means trust 1 may suggest, which is what the
    // old literal did. `<=` would exclude exactly the readers this replaces.
    let min_trust = state.config().roadmap.min_trust;
    if trust < min_trust {
        return Err(ApiError(AppError::Validation {
            // The bar is in the message. "trust level too low" is true and
            // useless; a reader who is told what would be enough can act on it.
            message: format!(
                "trust level too low to suggest: this instance needs trust level \
                 {min_trust} and yours is {trust}"
            ),
            field_errors: BTreeMap::new(),
        }));
    }

    // Bound the body BEFORE the trust lookup's result is used for anything
    // else, and refuse rather than truncate: a silently shortened description
    // reads as a complete one.
    if let Some(ref text) = body.body {
        if text.chars().count() > MAX_SUGGESTED_BODY {
            return Err(ApiError(AppError::Validation {
                message: format!("description must be {MAX_SUGGESTED_BODY} characters or fewer"),
                field_errors: BTreeMap::new(),
            }));
        }
    }

    let card = roadmap::find_card_by_title_normalized(state.db(), &body.title)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;

    let card_id = card.as_ref().map(|c| c.id.clone());
    roadmap::insert_suggestion(
        state.db(),
        &account_id,
        &body.title,
        body.body.as_deref(),
        card_id.as_deref(),
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e.into())))?;

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "status": "recorded",
            "matched_existing": card_id.is_some(),
        })),
    ))
}

/// `POST /api/v1/admin/roadmap/move` — move a card to a new stage.
#[derive(Debug, Deserialize)]
pub struct MoveBody {
    pub card_id: String,
    pub stage: String,
    pub reason: String,
}

pub async fn post_move_card(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Json(body): Json<MoveBody>,
) -> ApiResult<Json<Value>> {
    // Operator-only.
    let account_id = user.account_id.to_string();
    let is_operator = governance::is_operator(state.db(), &account_id)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    if !is_operator {
        return Err(ApiError(AppError::AccessDenied));
    }

    let stage = body.stage.as_str();
    if !darkhold_domain::consensus::STAGES.contains(&stage) {
        return Err(ApiError(AppError::Validation {
            message: format!("invalid stage: {stage}"),
            field_errors: BTreeMap::new(),
        }));
    }

    let cards = roadmap::list_cards(state.db(), None)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    let card = cards
        .iter()
        .find(|c| c.id == body.card_id)
        .ok_or_else(|| ApiError(AppError::NotFound { resource: "card" }))?;

    let from_stage = card.stage.clone();
    roadmap::record_move(
        state.db(),
        &body.card_id,
        &from_stage,
        stage,
        &body.reason,
        &account_id,
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e.into())))?;

    // Update the card's stage directly.
    roadmap::update_card_stage(state.db(), &body.card_id, stage)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;

    Ok(Json(json!({ "status": "moved" })))
}

/// `GET /api/v1/roadmap/changelog` — public feed of stage moves.
pub async fn get_changelog(
    State(state): State<AppState>,
    MaybeSession(_session): MaybeSession,
) -> ApiResult<Json<Value>> {
    let moves = roadmap::list_moves(state.db(), 50, 0)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    Ok(Json(json!({
        "moves": moves.iter().map(|(card_id, from, to, reason, moved_by, at)| {
            json!({
                "card_id": card_id,
                "from_stage": from,
                "to_stage": to,
                "reason": reason,
                "moved_by": moved_by,
                "created_at": at,
            })
        }).collect::<Vec<_>>()
    })))
}
