//! M17 — Translation routes: jobs, units, memory, glossaries, reviews, publications.

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use std::str::FromStr;

use crate::auth::MaybeSession;
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

/// Create a translation job.
#[derive(Debug, Deserialize)]
pub struct CreateTranslationBody {
    pub source_lang: String,
    pub target_lang: String,
    pub provider: String,
}

pub async fn create_translation(
    State(state): State<AppState>,
    Path(work_id): Path<String>,
    MaybeSession(user): MaybeSession,
    Json(body): Json<CreateTranslationBody>,
) -> ApiResult<Json<Value>> {
    let account = user.map(|u| u.account_id.to_string()).unwrap_or_default();
    if account.is_empty() {
        return Err(ApiError(darkhold_domain::AppError::field(
            "session",
            "sign in to request translations",
        )));
    }

    // §40.2: "the translation pipeline (M17) checks `translation` before a request is created".
    // Measured 2026-11-02: this door created a job without ever reading the statement, so the
    // enforcement sentence was a spec claim with nothing behind it (M45-30).
    let permission = darkhold_db::permission::get_work_permission_statement(state.db(), &work_id)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;
    match darkhold_domain::translation::funding_gate(permission.translation) {
        darkhold_domain::translation::Funding::Fundable => {}
        darkhold_domain::translation::Funding::MustAsk => {
            return Err(ApiError(darkhold_domain::AppError::field(
                "translation",
                "the author has asked to be consulted before a translation is made: this build \
                 does not create a request it cannot get an answer to (the `ask` workflow, §40.2)",
            )))
        }
        darkhold_domain::translation::Funding::Forbidden { statement } => {
            return Err(ApiError(darkhold_domain::AppError::field(
                "translation",
                format!(
                    "this work's translation statement is `{statement}`: the author has not \
                     allowed translations"
                ),
            )))
        }
    }

    let id = darkhold_db::translation::create_job(
        state.db(),
        &work_id,
        &body.source_lang,
        &body.target_lang,
        &body.provider,
        None,
        &account,
    )
    .await
    .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;

    Ok(Json(json!({ "id": id })))
}

/// Get translation job status.
pub async fn get_translation(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
    MaybeSession(_user): MaybeSession,
) -> ApiResult<Json<Value>> {
    let job = darkhold_db::translation::get_job(state.db(), &job_id)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;

    match job {
        Some(j) => Ok(Json(j)),
        None => Err(ApiError(darkhold_domain::AppError::field(
            "job",
            "not found",
        ))),
    }
}

/// Transition a translation job.
#[derive(Debug, Deserialize)]
pub struct TransitionJobBody {
    pub from_state: String,
    pub to_state: String,
}

pub async fn transition_translation(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
    MaybeSession(_user): MaybeSession,
    Json(body): Json<TransitionJobBody>,
) -> ApiResult<Json<Value>> {
    let from = darkhold_domain::translation::TranslationJobState::from_str(&body.from_state)
        .map_err(|e| ApiError(darkhold_domain::AppError::field("from_state", &e)))?;
    let to = darkhold_domain::translation::TranslationJobState::from_str(&body.to_state)
        .map_err(|e| ApiError(darkhold_domain::AppError::field("to_state", &e)))?;

    darkhold_db::translation::transition_job(state.db(), &job_id, &from, &to)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;

    Ok(Json(json!({ "success": true })))
}

/// List translation memory entries for the caller.
pub async fn list_memory(
    State(state): State<AppState>,
    MaybeSession(user): MaybeSession,
) -> ApiResult<Json<Value>> {
    let account = user.map(|u| u.account_id.to_string()).unwrap_or_default();
    if account.is_empty() {
        return Err(ApiError(darkhold_domain::AppError::field(
            "session",
            "sign in to list translation memory",
        )));
    }
    let memory = darkhold_db::translation::list_memory(state.db(), &account)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;
    Ok(Json(json!({ "memory": memory })))
}

/// Add glossary term.
#[derive(Debug, Deserialize)]
pub struct AddGlossaryBody {
    pub source_lang: String,
    pub target_lang: String,
    pub term: String,
    pub translation: String,
    pub case_sensitive: Option<bool>,
}

pub async fn add_glossary_term(
    State(state): State<AppState>,
    MaybeSession(user): MaybeSession,
    Json(body): Json<AddGlossaryBody>,
) -> ApiResult<Json<Value>> {
    let account = user.map(|u| u.account_id.to_string()).unwrap_or_default();
    if account.is_empty() {
        return Err(ApiError(darkhold_domain::AppError::field(
            "session",
            "sign in to manage glossaries",
        )));
    }

    let id = darkhold_db::translation::add_glossary_term(
        state.db(),
        &account,
        None,
        &body.source_lang,
        &body.target_lang,
        &body.term,
        &body.translation,
        body.case_sensitive.unwrap_or(false),
    )
    .await
    .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;

    Ok(Json(json!({ "id": id })))
}

/// Open a review gate.
#[derive(Debug, Deserialize)]
pub struct OpenReviewBody {
    pub reviewer: String,
    pub gate: String,
}

pub async fn open_review(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
    MaybeSession(_user): MaybeSession,
    Json(body): Json<OpenReviewBody>,
) -> ApiResult<Json<Value>> {
    let gate = darkhold_domain::translation::ReviewGate::from_str(&body.gate)
        .map_err(|e| ApiError(darkhold_domain::AppError::field("gate", &e)))?;

    let id = darkhold_db::translation::open_review_gate(state.db(), &job_id, &body.reviewer, &gate)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;

    Ok(Json(json!({ "id": id })))
}

/// Decide a review gate.
#[derive(Debug, Deserialize)]
pub struct DecideReviewBody {
    pub state: String,
    pub notes: Option<String>,
}

pub async fn decide_review(
    State(state): State<AppState>,
    Path(review_id): Path<String>,
    MaybeSession(_user): MaybeSession,
    Json(body): Json<DecideReviewBody>,
) -> ApiResult<Json<Value>> {
    darkhold_db::translation::decide_review_gate(
        state.db(),
        &review_id,
        &body.state,
        body.notes.as_deref(),
    )
    .await
    .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;

    Ok(Json(json!({ "success": true })))
}

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/works/{id}/translations", post(create_translation))
        .route("/translations/{job_id}", get(get_translation))
        .route(
            "/translations/{job_id}/transition",
            post(transition_translation),
        )
        .route("/me/translation-memory", get(list_memory))
        .route("/me/translation-glossaries", post(add_glossary_term))
        .route("/translations/{job_id}/reviews", post(open_review))
        .route(
            "/translation-reviews/{review_id}/decide",
            post(decide_review),
        )
        // §61 (M45-30) — the supply report and the funding door.
        .route("/translations/supply", get(supply))
        .route("/translations/bounties", get(list_translation_bounties))
        .route("/works/{id}/translation-bounty", post(fund_translation))
}

/// The languages this instance holds, and whether C6's taste transfer can be computed here.
///
/// The transfer half answers `available: false` with the two measured gaps named: this tree has no
/// stylometry (`grep -rln stylometr crates/` finds a catalogue string and nothing else), and
/// `work_tags` is not backfilled (M63-32). A cross-language taste score over that would be a number
/// with no signal behind it.
pub async fn supply(
    State(state): State<AppState>,
    MaybeSession(_session): MaybeSession,
) -> ApiResult<Json<Value>> {
    let supply = darkhold_db::translation::language_supply(state.db())
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;
    // C6 names the communities it cares about; the report says which are missing rather than
    // leaving a reader to compare counts by eye.
    let wanted = ["pt", "es", "fr", "ru", "zh", "ja", "ko", "id"];
    let unrepresented = darkhold_domain::translation::unrepresented(&supply, &wanted);
    let transfer = darkhold_domain::translation::transfer_available(false, false);
    let (transfer_available, missing) = match &transfer {
        darkhold_domain::translation::Transfer::Available => (true, Vec::new()),
        darkhold_domain::translation::Transfer::Unavailable { missing } => (false, missing.clone()),
    };
    Ok(Json(json!({
        "by_language": supply.by_language,
        "total": supply.total,
        "unrepresented": unrepresented,
        "transfer": { "available": transfer_available, "missing": missing },
    })))
}

pub async fn list_translation_bounties(
    State(state): State<AppState>,
    MaybeSession(_session): MaybeSession,
) -> ApiResult<Json<Value>> {
    let bounties = darkhold_db::translation::translation_bounties(state.db(), 50)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;
    Ok(Json(
        json!({ "bounties": bounties, "count": bounties.len() }),
    ))
}

#[derive(Debug, Deserialize)]
pub struct FundTranslationBody {
    /// The language the patron wants the work translated into.
    pub target_language: String,
    /// Basis points, as the economy stores amounts.
    pub amount: i64,
}

/// Fund a translation of a work.
///
/// The gate is the author's `translation` statement, read through §40.2's model, and it is checked
/// **before** the balance: a refusal that named a balance first would tell a patron to top up for
/// something the author has forbidden.
pub async fn fund_translation(
    State(state): State<AppState>,
    Path(work_id): Path<String>,
    MaybeSession(user): MaybeSession,
    Json(body): Json<FundTranslationBody>,
) -> ApiResult<Json<Value>> {
    let Some(user) = user else {
        return Err(ApiError(darkhold_domain::AppError::field(
            "session",
            "sign in to fund a translation",
        )));
    };
    let target = darkhold_domain::translation::normalise_language(&body.target_language);
    let Some(target) = target else {
        return Err(ApiError(darkhold_domain::AppError::field(
            "target_language",
            "a translation needs a language tag to translate into, such as `pt-BR` or `es`",
        )));
    };
    if body.amount <= 0 {
        return Err(ApiError(darkhold_domain::AppError::field(
            "amount",
            "a bounty is a promise of credits: it must be greater than zero",
        )));
    }

    let outcome = darkhold_db::translation::fund_translation(
        state.db(),
        &work_id,
        &target,
        &user.account_id.to_string(),
        body.amount,
    )
    .await
    .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;

    match outcome {
        darkhold_db::translation::FundOutcome::Funded { bounty_id } => Ok(Json(json!({
            "funded": true,
            "bounty_id": bounty_id,
            "target_language": target,
            "amount": body.amount,
        }))),
        darkhold_db::translation::FundOutcome::MustAsk => {
            Err(ApiError(darkhold_domain::AppError::field(
                "translation",
                "the author has asked to be consulted before translations are made: funding one \
                 now would be spending their silence",
            )))
        }
        darkhold_db::translation::FundOutcome::Forbidden { statement } => {
            Err(ApiError(darkhold_domain::AppError::field(
                "translation",
                format!(
                    "this work's translation statement is `{statement}`: the author has not \
                     allowed translations, so there is nothing to fund"
                ),
            )))
        }
        darkhold_db::translation::FundOutcome::InsufficientBalance {
            available,
            required,
        } => Err(ApiError(darkhold_domain::AppError::field(
            "amount",
            format!(
                "the bounty promises {required} and this account holds {available}: a bounty \
                     the account cannot pay is a promise the archive would have to break"
            ),
        ))),
    }
}
