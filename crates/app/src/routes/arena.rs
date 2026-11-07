//! Taste Calibration Arena routes (spec §0.4.2a).
//!
//! The arena presents 4 works sharing at least one major attribute (fandom,
//! genre, or length bracket). The reader picks best and worst, and the
//! forced tradeoff reveals which dimensions of taste matter most.

use crate::auth::RequireSession;
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;
use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use darkhold_db;
use darkhold_domain::arena_pairing::{select_round, ArenaSignal, PairingConfig, PairingMode};
use darkhold_domain::taste_vector::{
    apply_arena_ballot, weights_from_elos, ArenaBallot, ArenaCard, ArenaRound, DimensionElo,
    TasteDimension,
};
use darkhold_domain::AppError;
use serde::{Deserialize, Serialize};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/arena/next", get(get_arena_next))
        .route("/arena/vote", post(post_arena_vote))
        .route("/arena/dismiss", post(post_arena_dismiss))
        .route("/arena/weights", get(get_arena_weights))
}

/// Response for GET /arena/next.
///
/// `round` is **optional on purpose**. `generate_arena_round` returns `None` for an
/// ordinary state — a reader who has rated nothing, on an instance whose works do
/// not yet share a fandom in fours — and that used to be reported as
/// `AppError::Internal`, i.e. a 500. It is not a server fault: it is the answer to
/// "is there a round for this reader right now?", and the answer is frequently no.
/// §0.4.2a's arena is a calibration aid, not a door, so an absent round is `200`
/// with an explanation rather than a failure string (the same rule §54.6 states for
/// the concierge, and the same rule `100-ideas-remaining.md` §3 states for every
/// rail: an empty answer renders as nothing, never as `Internal`).
#[derive(Debug, Serialize)]
pub struct ArenaNextResponse {
    pub round: Option<ArenaRound>,
    /// Why there is no round, when there is none. Absent whenever `round` is.
    ///
    /// Kept separate from `round: null` being the *only* signal so the client can
    /// tell a reader with an empty pool from a reader who has exhausted every
    /// pair worth asking about.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explained_empty: Option<String>,
    /// How these four were chosen: `proximate` (close on the ladder, a comparison that sharpens what is believed) or
    /// `explore` (spread apart, a comparison that locates a work). Absent when there is no round.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<PairingMode>,
    /// Cards still inside their calibration window. Their ballots are recorded with that fact and their ratings are not
    /// treated as the ladder's, so a page can say "still calibrating" instead of presenting a first reader as a verdict.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub calibrating: Vec<String>,
    /// One sentence: why these four. Absent when there is no round.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub dimensions: Vec<DimensionSummary>,
}

/// A dimension summary shown to the user (never raw weights).
#[derive(Debug, Serialize)]
pub struct DimensionSummary {
    pub key: String,
    pub label: String,
    pub matches_played: u64,
}

/// Request body for POST /arena/vote.
#[derive(Debug, Deserialize)]
pub struct ArenaVoteRequest {
    pub best_work_id: String,
    pub worst_work_id: String,
    #[serde(default)]
    pub reason_tags: Vec<String>,
    /// The cards the reader was actually shown, so the ballot can be scored as the five
    /// comparisons it is (spec §12.4a). Optional and usually four: a client that omits it
    /// still rates best against worst, which is one comparison instead of five. The field is
    /// not required because an old bundle is not a reason to reject a vote — but a vote whose
    /// middles are missing is four works rated by two, and the response says how many
    /// comparisons were recorded so the difference is visible rather than assumed.
    #[serde(default)]
    pub card_ids: Vec<String>,
}

/// Response for POST /arena/vote.
#[derive(Debug, Serialize)]
pub struct ArenaVoteResponse {
    pub success: bool,
    pub message: String,
    pub next_round: Option<ArenaRound>,
}

/// Response for GET /arena/weights.
#[derive(Debug, Serialize)]
pub struct ArenaWeightsResponse {
    pub dimensions: Vec<DimensionWeightSummary>,
}

/// A dimension weight summary (never the raw weight value).
#[derive(Debug, Serialize)]
pub struct DimensionWeightSummary {
    pub key: String,
    pub label: String,
    /// Qualitative label, never a raw number (spec §0.3).
    pub influence: String,
}

/// Build the arena's card pool from the DB rows, with each work's real taste
/// vector attached.
///
/// The vector is what the arena varies on, so it has to come from the same
/// scoring the rest of the recommender uses. `get_arena_pool` returns the
/// stored taste vector alongside each work; when one is missing the card falls
/// back to the neutral vector rather than a fabricated score.
fn build_cards(
    rows: Vec<darkhold_db::taste_vectors::ArenaPoolRow>,
    dimensions: &[TasteDimension],
) -> Vec<ArenaCard> {
    let dimension_tuples: Vec<(String, String, f64, f64)> = dimensions
        .iter()
        .map(|d| (d.key.clone(), d.label.clone(), d.admin_target, d.weight))
        .collect();

    // The round varies on the reader's least-confident dimension: a dimension
    // the model has no opinion about is the one worth learning.
    let target_dimension = dimensions
        .first()
        .map(|d| d.key.clone())
        .unwrap_or_default();

    rows.into_iter()
        .map(|row| ArenaCard {
            work_id: row.id,
            title: row.title,
            fandom: row.fandom,
            tags: row.tags,
            word_count: row.word_count,
            excerpt: row.summary,
            target_dimension: target_dimension.clone(),
            vector: darkhold_domain::taste_vector::work_vector_from_tags(
                &dimension_tuples,
                &row.tag_weights,
            ),
            // Filled by `attach_signals`: the pool query knows the catalogue, the ladder is another process.
            signal: None,
        })
        .collect()
}

/// The pairing knobs, with the epsilon the instance configured.
fn pairing_config(state: &AppState) -> PairingConfig {
    PairingConfig {
        epsilon: state.config().discovery.arena_epsilon,
        ..PairingConfig::default()
    }
}

/// Attach the ladder's reading to each card, and say which cards are still calibrating.
///
/// Two sources, in this order: the estate taste service (`state.taste().ratings`) when it answers, and the arena's own
/// ballot count (`taste_vectors::arena_appearances`) for how many comparisons a work has actually had. The service's
/// `matches` is used when present because it is the count the rating was computed from; the local count is the floor
/// when it is absent.
///
/// **A failure here does not fail the round.** Pairing without readings is still pairing — every card is treated as
/// unmeasured, so the round is `proximate` over the starting value and every card is marked calibrating. That is a
/// worse round than a measured one and it is not an error: an outage in another process must cost a sharper question,
/// never the arena.
async fn attach_signals(state: &AppState, cards: &mut [ArenaCard]) {
    if cards.is_empty() {
        return;
    }
    let ids: Vec<String> = cards.iter().map(|card| card.work_id.clone()).collect();
    let appearances = darkhold_db::taste_vectors::arena_appearances(state.db(), &ids)
        .await
        .unwrap_or_default();

    let readings = match state.taste().ratings(2_000).await {
        Ok(ratings) => ratings,
        Err(error) => {
            tracing::debug!(%error, "the taste service did not answer; pairing without readings");
            Vec::new()
        }
    };
    let by_work: std::collections::HashMap<String, crate::taste::Rating> = readings
        .into_iter()
        .map(|rating| {
            let id = rating
                .item_id
                .strip_prefix("work:")
                .unwrap_or(&rating.item_id)
                .to_owned();
            (id, rating)
        })
        .collect();

    for card in cards.iter_mut() {
        let local = appearances.get(&card.work_id).copied().unwrap_or(0);
        card.signal = by_work.get(&card.work_id).map(|rating| ArenaSignal {
            rating: rating.rating,
            rd: Some(rating.deviation),
            matches: u64::try_from(rating.matches.max(local)).unwrap_or(u64::MAX),
        });
        if card.signal.is_none() {
            card.signal = Some(ArenaSignal {
                rating: darkhold_domain::arena_pairing::DEFAULT_RATING,
                rd: None,
                matches: u64::try_from(local).unwrap_or(u64::MAX),
            });
        }
    }
}

/// GET /arena/next — the next arena round for the signed-in user.
async fn get_arena_next(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<ArenaNextResponse>> {
    let db = state.db();
    let account_id = user.account_id.to_string();

    // Get the account's taste dimensions (or defaults).
    let dimensions = get_account_dimensions(db, &account_id).await?;

    // Get works not yet voted on.
    let pool_rows = darkhold_db::taste_vectors::get_arena_pool(db, &account_id, 50)
        .await
        .map_err(|e| ApiError(AppError::Internal(anyhow::anyhow!("arena pool: {e}"))))?;

    let cards = build_cards(pool_rows, &dimensions);

    // Vary on every configured dimension: the round should offer the reader a
    // real tradeoff across their whole profile, not one arbitrary axis.
    let targets: Vec<String> = dimensions.iter().map(|d| d.key.clone()).collect();
    // `None` here is an ordinary state, not a fault: a reader who has rated
    // nothing, on an instance with fewer than four public works sharing a fandom,
    // has no round to be offered. It used to be `AppError::Internal` — a 500 — and
    // `/arena` opened with a red "That did not work" for every new account.
    //
    // The dimensions are still computed and reported either way, because they are
    // what the page shows about calibration progress, and an empty round must not
    // cost the reader that too.
    let mut cards = cards;
    attach_signals(&state, &mut cards).await;
    // The roll is drawn here and passed in, so the decision is testable: `select_round` owns the rule, not the coin.
    let roll = rand::random::<f64>();
    let pairing = select_round(&cards, &dimensions, &targets, pairing_config(&state), roll);
    let round = pairing.as_ref().map(|outcome| ArenaRound {
        cards: outcome.cards.clone(),
    });
    let explained_empty = round.is_none().then(|| {
        if cards.is_empty() {
            "No published, public works are available to compare yet."
        } else {
            "Not enough comparable works yet — an arena round needs four published \
             works that share a fandom."
        }
        .to_owned()
    });

    // Get existing Elo ratings for dimensions.
    let elos = get_dimension_elos(db, &account_id, &dimensions).await?;

    let dimension_summaries = dimensions
        .iter()
        .zip(elos.iter())
        .map(|(d, e)| DimensionSummary {
            key: d.key.clone(),
            label: d.label.clone(),
            matches_played: e.matches_played as u64,
        })
        .collect();

    Ok(Json(ArenaNextResponse {
        round,
        explained_empty,
        mode: pairing.as_ref().map(|outcome| outcome.mode),
        calibrating: pairing
            .as_ref()
            .map(|outcome| outcome.calibrating.clone())
            .unwrap_or_default(),
        reason: pairing.map(|outcome| outcome.reason),
        dimensions: dimension_summaries,
    }))
}

/// POST /arena/vote — submit an arena ballot.
async fn post_arena_vote(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Json(req): Json<ArenaVoteRequest>,
) -> ApiResult<Json<ArenaVoteResponse>> {
    let db = state.db();
    let account_id = user.account_id.to_string();

    // Validate best != worst.
    if req.best_work_id == req.worst_work_id {
        return Err(ApiError(AppError::Validation {
            message: "best and worst must be different".into(),
            field_errors: Default::default(),
        }));
    }

    // Whether either work is still being positioned. Read from the ladder before the ballot is stored, because the
    // count the window is measured against changes with this vote: computing it afterwards would call the ballot that
    // took a work over the line a calibrating one, or the reverse.
    let mut voted_cards = Vec::new();
    for id in [&req.best_work_id, &req.worst_work_id] {
        voted_cards.push(ArenaCard {
            work_id: id.clone(),
            title: String::new(),
            fandom: String::new(),
            tags: Vec::new(),
            word_count: 0,
            excerpt: String::new(),
            target_dimension: String::new(),
            vector: Vec::new(),
            signal: None,
        });
    }
    attach_signals(&state, &mut voted_cards).await;
    let calibrating = voted_cards.iter().any(|card| {
        card.signal
            .is_none_or(|signal| signal.calibrating(pairing_config(&state).calibration_matches))
    });

    // Record the ballot.
    darkhold_db::taste_vectors::record_arena_ballot(
        db,
        &account_id,
        &req.best_work_id,
        &req.worst_work_id,
        &req.reason_tags,
        calibrating,
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(anyhow::anyhow!("record ballot: {e}"))))?;

    // The works ladder (spec §12.4a) is now the estate's, not this crate's: the four cards the reader
    // saw are published as a pool and the vote is cast as one best/worst ballot at `polaris-taste`
    // (ADR 0001). The module that used to do this arithmetic here — 746 lines of Glicko and five
    // comparisons per ballot — is gone, because four products rating one person's taste four ways was
    // the defect the service exists to fix.
    //
    // A failure here does NOT fail the request, and the same reasoning as before still holds: the
    // ballot above is already stored locally and it is the reader's judgement. But the reason the
    // failure is tolerated has changed — the ladder is now a *different process*, so an outage there
    // must cost ratings and never a vote. It is logged at warn level so a dead service is visible.
    match state
        .taste()
        .cast_best_worst(
            &account_id,
            &req.card_ids,
            &req.best_work_id,
            &req.worst_work_id,
        )
        .await
    {
        Ok(cast) => {
            tracing::debug!(
                comparisons = cast.comparisons,
                "the arena ballot reached the estate taste service"
            );
        }
        Err(error) => {
            tracing::warn!(%error, "the taste service did not record this ballot");
        }
    }

    // Get current Elo ratings.
    let dimensions = get_account_dimensions(db, &account_id).await?;
    let elos = get_dimension_elos(db, &account_id, &dimensions).await?;

    // The ballot has to be scored against the works it was cast for, so rebuild
    // the round the reader actually saw. Passing an empty round — which is what
    // this used to do — left the Elo loop with no cards to compare and every
    // dimension frozen at 1500 forever.
    let ballots = darkhold_db::taste_vectors::get_arena_pool(db, &account_id, 200)
        .await
        .map_err(|e| ApiError(AppError::Internal(anyhow::anyhow!("arena pool: {e}"))))?;
    let cards = build_cards(ballots, &dimensions);
    let round = ArenaRound {
        cards: cards
            .into_iter()
            .filter(|card| card.work_id == req.best_work_id || card.work_id == req.worst_work_id)
            .collect(),
    };

    let ballot = ArenaBallot {
        best_work_id: req.best_work_id.clone(),
        worst_work_id: req.worst_work_id.clone(),
        reason_tags: req.reason_tags.clone(),
    };

    let updated_elos = apply_arena_ballot(&elos, &round, &ballot, &dimensions);

    // Store updated Elo ratings.
    let weight_pairs = weights_from_elos(&updated_elos);
    for (dim_key, weight) in &weight_pairs {
        let elo = updated_elos
            .iter()
            .find(|e| &e.dimension_key == dim_key)
            .unwrap();
        darkhold_db::taste_vectors::update_arena_weights(
            db,
            &account_id,
            dim_key,
            *weight,
            elo.elo_rating,
            elo.matches_played as i64,
        )
        .await
        .map_err(|e| ApiError(AppError::Internal(anyhow::anyhow!("update weights: {e}"))))?;
    }

    // Generate next round. The pool now excludes the works just voted on, so
    // the reader is never asked the same question twice.
    let pool_rows = darkhold_db::taste_vectors::get_arena_pool(db, &account_id, 50)
        .await
        .map_err(|e| ApiError(AppError::Internal(anyhow::anyhow!("arena pool: {e}"))))?;
    let mut cards = build_cards(pool_rows, &dimensions);
    attach_signals(&state, &mut cards).await;
    let targets: Vec<String> = dimensions.iter().map(|d| d.key.clone()).collect();
    let next_round = select_round(
        &cards,
        &dimensions,
        &targets,
        pairing_config(&state),
        rand::random::<f64>(),
    )
    .map(|outcome| ArenaRound {
        cards: outcome.cards,
    });

    Ok(Json(ArenaVoteResponse {
        success: true,
        message: "Ballot recorded.".into(),
        next_round,
    }))
}

/// POST /arena/dismiss — skip the arena (falls back to quiz or egalitarian).
async fn post_arena_dismiss(
    State(_state): State<AppState>,
    RequireSession(_user): RequireSession,
) -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(serde_json::json!({
        "success": true,
        "message": "Arena dismissed. Falling back to egalitarian recommendations."
    })))
}

/// GET /arena/weights — the signed-in user's arena weight summary.
async fn get_arena_weights(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<ArenaWeightsResponse>> {
    let db = state.db();
    let account_id = user.account_id.to_string();
    let dimensions = get_account_dimensions(db, &account_id).await?;
    let elos = get_dimension_elos(db, &account_id, &dimensions).await?;
    let weights = weights_from_elos(&elos);

    let summaries = dimensions
        .iter()
        .map(|d| {
            let weight = weights
                .iter()
                .find(|(k, _)| k == &d.key)
                .map(|(_, w)| *w)
                .unwrap_or(0.0);
            // Convert to qualitative label (never raw number, spec §0.3).
            let influence = if weight > 0.3 {
                "Strong"
            } else if weight > 0.15 {
                "Moderate"
            } else {
                "Weak"
            };
            DimensionWeightSummary {
                key: d.key.clone(),
                label: d.label.clone(),
                influence: influence.to_string(),
            }
        })
        .collect();

    Ok(Json(ArenaWeightsResponse {
        dimensions: summaries,
    }))
}

/// Get the account's taste dimensions (or defaults).
///
/// The fallback below is load-bearing, and the reason it used not to fire is worth
/// recording: `accounts.taste_vector` is `DEFAULT '[]'`, not NULL (migration 0053),
/// so **every** account has a row and `get_taste_vector` returns
/// `Some((vec![], ..))` rather than `None`. Mapping that empty vector gave a
/// reader with no quiz answers **zero** dimensions, and `generate_arena_round`
/// returns `None` when it is given no target dimensions — so every new account got
/// `Internal("not enough works for arena round")` → 500, on a seeded instance as
/// well as an empty one, and the message named works when the real problem was
/// dimensions.
///
/// So the test is `vec.is_empty()`, not `profile.is_some()`. A reader who has not
/// taken the quiz gets the instance's default dimensions, which is what the
/// `.unwrap_or_else` arm was written to do all along.
async fn get_account_dimensions(
    db: &darkhold_db::Database,
    account_id: &str,
) -> ApiResult<Vec<TasteDimension>> {
    // Try to get from taste profile.
    let profile = darkhold_db::taste_vectors::get_taste_vector(db, account_id)
        .await
        .map_err(|e| ApiError(AppError::Internal(anyhow::anyhow!("taste vector: {e}"))))?;

    let default_keys: Vec<String> = darkhold_domain::taste_vector::DEFAULT_DIMENSIONS
        .iter()
        .map(|s| s.to_string())
        .collect();

    let dims = profile
        .filter(|(vec, _, _)| !vec.is_empty())
        .map(|(vec, _, _)| {
            vec.iter()
                .enumerate()
                .map(|(i, _)| TasteDimension {
                    key: default_keys
                        .get(i)
                        .cloned()
                        .unwrap_or_else(|| format!("dim_{}", i)),
                    label: format!("Dimension {}", i + 1),
                    admin_target: 0.5,
                    weight: 1.0 / (vec.len() as f64).max(1.0),
                })
                .collect()
        })
        .unwrap_or_else(|| {
            darkhold_domain::taste_vector::DEFAULT_DIMENSIONS
                .iter()
                .map(|key| TasteDimension {
                    key: key.to_string(),
                    label: key.to_string(),
                    admin_target: 0.5,
                    weight: 1.0 / darkhold_domain::taste_vector::DEFAULT_DIMENSIONS.len() as f64,
                })
                .collect()
        });

    Ok(dims)
}

/// Get or create Elo ratings for dimensions.
async fn get_dimension_elos(
    db: &darkhold_db::Database,
    account_id: &str,
    dimensions: &[TasteDimension],
) -> ApiResult<Vec<DimensionElo>> {
    let rows = darkhold_db::taste_vectors::get_arena_weights(db, account_id)
        .await
        .map_err(|e| ApiError(AppError::Internal(anyhow::anyhow!("arena weights: {e}"))))?;

    let elos = dimensions
        .iter()
        .map(|d| {
            let row = rows.iter().find(|(key, _, _, _)| key == &d.key);
            DimensionElo {
                dimension_key: d.key.clone(),
                elo_rating: row.map(|(_, _, elo, _)| *elo).unwrap_or(1500.0),
                matches_played: row.map(|(_, _, _, mp)| *mp as u32).unwrap_or(0),
            }
        })
        .collect();

    Ok(elos)
}
