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
    MaybeSession(user): MaybeSession,
) -> ApiResult<Json<Value>> {
    let account = user.map(|u| u.account_id.to_string()).unwrap_or_default();
    if account.is_empty() {
        return Err(ApiError(darkhold_domain::AppError::field(
            "session",
            "sign in to read a translation job",
        )));
    }

    let job = darkhold_db::translation::get_job(state.db(), &job_id)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;

    // Scoped, where it used to answer anybody with a job id. A translation job carries the provider, the quote and the
    // language pair of somebody's work, and `get_job` has no idea who asked: the check belongs here, and the refusal is
    // 404 rather than 403 because confirming that a job exists is itself a disclosure (ADR 0030).
    match job {
        Some(j) if j.get("created_by").and_then(Value::as_str) == Some(account.as_str()) => Ok(Json(j)),
        _ => Err(ApiError(darkhold_domain::AppError::NotFound {
            resource: "translation",
        })),
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
        // M70-13: the job's own units, its publication, and the memory pair (write + lookup) beside the existing list.
        .route(
            "/translations/{job_id}/units",
            get(list_units).post(record_unit),
        )
        .route("/translations/{job_id}/publish", post(publish_translation))
        .route(
            "/me/translation-memory",
            post(add_memory_entry),
        )
        .route("/me/translation-memory/lookup", get(lookup_memory_entry))
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

// ---------------------------------------------------------------------------
// The job's units, its publication, and translation memory (M70-13)
// ---------------------------------------------------------------------------

/// Confirm the job exists and belongs to the caller, or answer 404.
///
/// Every door below needs the job's `source_work` and `state`, and every one of them must answer a stranger the same way
/// `get_translation` now does. One helper, so the refusal cannot drift between four routes.
async fn owned_job(state: &AppState, account: &str, job_id: &str) -> ApiResult<Value> {
    let job = darkhold_db::translation::get_job(state.db(), job_id)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;
    match job {
        Some(job) if job.get("created_by").and_then(Value::as_str) == Some(account) => Ok(job),
        _ => Err(ApiError(darkhold_domain::AppError::NotFound {
            resource: "translation",
        })),
    }
}

fn account_of(user: Option<crate::auth::SessionUser>) -> ApiResult<String> {
    let account = user.map(|u| u.account_id.to_string()).unwrap_or_default();
    if account.is_empty() {
        return Err(ApiError(darkhold_domain::AppError::field(
            "session",
            "sign in to work on a translation",
        )));
    }
    Ok(account)
}

/// The translated paragraphs recorded for one job.
///
/// A job in progress is a list of paragraphs, each with its own state: one job can have three paragraphs approved and
/// four still pending, and a caller that only sees the job's state cannot tell.
async fn list_units(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
    MaybeSession(user): MaybeSession,
) -> ApiResult<Json<Value>> {
    let account = account_of(user)?;
    owned_job(&state, &account, &job_id).await?;
    let units = darkhold_db::translation::units_for_job(state.db(), &job_id)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;
    Ok(Json(json!({ "job_id": job_id, "units": units })))
}

/// Record one paragraph's translation.
#[derive(Debug, Deserialize)]
pub struct RecordUnitBody {
    /// The source unit the paragraph belongs to.
    pub unit_id: String,
    /// Which paragraph of that unit.
    pub paragraph_index: i64,
    /// The source sentence, as it was translated from.
    pub source_text: String,
    /// The translated sentence, when there is one.
    pub target_text: Option<String>,
    /// `pending`, `translated`, `reviewed` or `approved`.
    pub state: String,
    /// The memory row this came from, when it came from memory rather than a person.
    pub memory_hit: Option<String>,
}

async fn record_unit(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
    MaybeSession(user): MaybeSession,
    Json(body): Json<RecordUnitBody>,
) -> ApiResult<Json<Value>> {
    let account = account_of(user)?;
    owned_job(&state, &account, &job_id).await?;

    let unit_state =
        darkhold_domain::translation::TranslationUnitState::from_str(&body.state).map_err(|error| {
            let mut field_errors = std::collections::BTreeMap::new();
            field_errors.insert("state".to_string(), error);
            ApiError(darkhold_domain::AppError::Validation {
                message: "unknown translation unit state".to_string(),
                field_errors,
            })
        })?;

    // A paragraph cannot be "translated" with nothing in it: the state is a claim about the text beside it, and accepting
    // the claim without the text would put a paragraph in the reviewed pile that nobody can review.
    if unit_state != darkhold_domain::translation::TranslationUnitState::Pending
        && body.target_text.as_deref().is_none_or(|text| text.trim().is_empty())
    {
        return Err(ApiError(darkhold_domain::AppError::field(
            "target_text",
            "a translated paragraph needs its translation",
        )));
    }

    darkhold_db::translation::upsert_unit(
        state.db(),
        &job_id,
        &body.unit_id,
        body.paragraph_index,
        &body.source_text,
        body.target_text.as_deref(),
        &unit_state,
        body.memory_hit.as_deref(),
    )
    .await
    .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;

    Ok(Json(json!({
        "job_id": job_id,
        "unit_id": body.unit_id,
        "paragraph_index": body.paragraph_index,
        "state": unit_state.as_str(),
    })))
}

/// Publish an approved translation.
///
/// The gate is the job's state, and it is a refusal rather than a coercion: §40's pipeline publishes from `approved`, and
/// a door that accepted a publish from `in_progress` would let a half-translated work appear as a finished one. The
/// publication row is written *after* the transition, because a publication row for a job that never moved would be a
/// claim about an event that did not happen.
async fn publish_translation(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
    MaybeSession(user): MaybeSession,
) -> ApiResult<Json<Value>> {
    let account = account_of(user)?;
    let job = owned_job(&state, &account, &job_id).await?;
    let current = job.get("state").and_then(Value::as_str).unwrap_or_default();
    if current != "approved" {
        return Err(ApiError(darkhold_domain::AppError::Validation {
            message: format!(
                "a translation is published from `approved`, and this job is `{current}`"
            ),
            field_errors: Default::default(),
        }));
    }
    let from = darkhold_domain::translation::TranslationJobState::Approved;
    let to = darkhold_domain::translation::TranslationJobState::Published;
    darkhold_db::translation::transition_job(state.db(), &job_id, &from, &to)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;

    let work_id = job
        .get("source_work")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let publication =
        darkhold_db::translation::record_publication(state.db(), &job_id, &work_id)
            .await
            .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;

    Ok(Json(json!({
        "job_id": job_id,
        "work_id": work_id,
        "publication_id": publication,
        "state": to.as_str(),
    })))
}

/// Add a sentence pair to the caller's translation memory.
#[derive(Debug, Deserialize)]
pub struct AddMemoryBody {
    pub source_lang: String,
    pub target_lang: String,
    pub source_text: String,
    pub target_text: String,
    /// Quality in basis points, 0–10000.
    #[serde(default)]
    pub quality_bp: i64,
    /// Whether other translators may reuse this pair.
    #[serde(default)]
    pub shared: bool,
}

/// The key a sentence pair is stored under: SHA-256 of the source text.
///
/// The hash is computed here rather than accepted from the client, because the writer and the reader must agree on it
/// exactly. A client that normalised differently — trimmed, lowercased, collapsed whitespace — would store a pair that
/// its own lookup could never find, and the failure would look like "memory is not working" rather than "two hashings".
fn memory_key(source_text: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(source_text.trim().as_bytes()))
}

async fn add_memory_entry(
    State(state): State<AppState>,
    MaybeSession(user): MaybeSession,
    Json(body): Json<AddMemoryBody>,
) -> ApiResult<Json<Value>> {
    let account = account_of(user)?;
    for (field, value) in [
        ("source_lang", &body.source_lang),
        ("target_lang", &body.target_lang),
        ("source_text", &body.source_text),
        ("target_text", &body.target_text),
    ] {
        if value.trim().is_empty() {
            return Err(ApiError(darkhold_domain::AppError::field(field, "must not be empty")));
        }
    }
    if !(0..=10_000).contains(&body.quality_bp) {
        return Err(ApiError(darkhold_domain::AppError::field(
            "quality_bp",
            "between 0 and 10000",
        )));
    }
    let source_hash = memory_key(&body.source_text);
    let id = darkhold_db::translation::add_memory(
        state.db(),
        &account,
        &body.source_lang,
        &body.target_lang,
        &source_hash,
        body.source_text.trim(),
        &body.target_text,
        body.quality_bp,
        body.shared,
    )
    .await
    .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;
    Ok(Json(json!({ "id": id, "source_hash": source_hash })))
}

/// What the caller's memory has for this sentence, if anything.
///
/// The lookup takes the sentence, not a hash: the client that stores a pair should not have to reproduce the writer's
/// key, and one hashing function is the only way the two can be guaranteed to agree.
async fn lookup_memory_entry(
    State(state): State<AppState>,
    MaybeSession(user): MaybeSession,
    axum::extract::Query(query): axum::extract::Query<LookupQuery>,
) -> ApiResult<Json<Value>> {
    let account = account_of(user)?;
    for (field, value) in [
        ("source_lang", &query.source_lang),
        ("target_lang", &query.target_lang),
        ("source_text", &query.source_text),
    ] {
        if value.trim().is_empty() {
            return Err(ApiError(darkhold_domain::AppError::field(field, "must not be empty")));
        }
    }
    let source_hash = memory_key(&query.source_text);
    let hit = darkhold_db::translation::lookup_memory(
        state.db(),
        &account,
        &query.source_lang,
        &query.target_lang,
        &source_hash,
    )
    .await
    .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;
    Ok(Json(match hit {
        Some((target_text, quality_bp, shared)) => json!({
            "source_hash": source_hash,
            "target_text": target_text,
            "quality_bp": quality_bp,
            "shared": shared,
        }),
        None => json!({ "source_hash": source_hash, "target_text": Value::Null }),
    }))
}

/// The lookup's query string.
#[derive(Debug, Deserialize)]
pub struct LookupQuery {
    pub source_lang: String,
    pub target_lang: String,
    pub source_text: String,
}
