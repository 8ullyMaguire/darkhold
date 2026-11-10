//! Imported taste signals (M45-20, §49.6–§49.8) — the module's first doors.
//!
//! `crates/db/src/taste_import.rs` was written with a long header explaining its five decisions and then had no route at
//! all: nothing in the application named `taste_import`, so a reader could not import a history, could not see the split
//! between what they gave this instance and what was carried over, and could not read the audit row of an import that
//! had (not) happened. Four doors, and the rule each one keeps:
//!
//! * **An import is always `imported`.** The store's `import_signals` takes no origin parameter, so an import cannot
//!   masquerade as organic at this call site — and §49.7's "distinguishable everywhere" only means something if the
//!   distinction survives every read, which is why `origins` reports both the counts *and* the discounted weight.
//! * **Re-importing the same history adds nothing.** The outcome's `added` is the number of rows actually inserted, so
//!   the second run of the same file is `added == 0` — a fact the client can check rather than a promise.
//! * **An import is the caller's.** Every read is scoped by the caller's account, and `import_run` — which returns the
//!   row including `account_id` — is filtered here rather than trusted.
//! * **A credential named in an import must be the caller's own.** `credential_id` is an association with a stored
//!   login; accepting one from the body without checking would let a reader attach somebody else's credential id to
//!   their own import run, which is a correlation neither of them asked for.

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use darkhold_db::taste_import::{self, IncomingSignal, SignalKind};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::{MaybeSession, RequirePseud};
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

/// The largest history this door accepts in one request.
///
/// The body limit is about bytes; this is about rows. A five-thousand-signal import is a real history from a busy
/// account, and anything past it is a client sending a file it should have split — refused with a number in the message
/// rather than silently truncated.
const MAX_SIGNALS_PER_IMPORT: usize = 5_000;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/me/taste/signals", get(list_signals))
        .route("/me/taste/signals/import", post(import_signals_route))
        .route("/me/taste/signals/origins", get(origin_split_route))
        .route("/me/taste/imports/{id}", get(import_run_route))
}

fn account_of(user: Option<crate::auth::SessionUser>) -> ApiResult<String> {
    let account = user.map(|u| u.account_id.to_string()).unwrap_or_default();
    if account.is_empty() {
        return Err(ApiError(darkhold_domain::AppError::field(
            "session",
            "sign in to read your taste profile",
        )));
    }
    Ok(account)
}

/// One signal, as the read returns it.
///
/// `effective_value` is included because it is what actually moves a weight, and a reader comparing an imported
/// bookmark to a rating given today should see the discount rather than infer it from a constant in the docs.
fn signal_view(signal: &taste_import::TasteSignal) -> Value {
    json!({
        "id": signal.id,
        "origin": signal.origin,
        "kind": signal.kind,
        "dimension_key": signal.dimension_key,
        "signal_value": signal.signal_value,
        "effective_value": signal.effective_value,
        "occurred_at": signal.occurred_at,
        "source_key": signal.source_key,
        "source_signal_key": signal.source_signal_key,
        "created_at": signal.created_at,
    })
}

/// The caller's signals, optionally narrowed to one dimension.
///
/// Two reads stand behind this and the choice is the query string's: with a `dimension_key` the answer is that
/// dimension's signals, without one it is everything the reader has, ordered by provenance so the imported block is
/// not interleaved with the organic one.
#[derive(Debug, Deserialize)]
pub struct SignalQuery {
    pub dimension_key: Option<String>,
}

async fn list_signals(
    State(state): State<AppState>,
    MaybeSession(user): MaybeSession,
    Query(query): Query<SignalQuery>,
) -> ApiResult<Json<Value>> {
    let account = account_of(user)?;
    let signals = match query
        .dimension_key
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty())
    {
        Some(dimension) => {
            taste_import::signals_for_dimension(state.db(), &account, dimension).await
        }
        None => taste_import::signals_by_origin(state.db(), &account).await,
    }
    .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;

    let items: Vec<Value> = signals.iter().map(signal_view).collect();
    Ok(Json(json!({
        "dimension_key": query.dimension_key,
        "count": items.len(),
        "signals": items,
    })))
}

/// One signal, as an importer offers it.
#[derive(Debug, Deserialize)]
pub struct IncomingSignalBody {
    /// `bookmark`, `kudos`, `rating` or `read`.
    pub kind: String,
    pub dimension_key: String,
    /// The source's own stable id for this item. Required: the import's idempotency is keyed on it.
    pub source_signal_key: String,
    #[serde(default)]
    pub occurred_at: Option<String>,
    #[serde(default)]
    pub provenance_json: Option<String>,
}

/// A history, offered by an importer.
#[derive(Debug, Deserialize)]
pub struct ImportBody {
    /// Which source the history came from, e.g. `ao3`.
    pub source_key: String,
    /// What kind of import this is, e.g. `bookmarks`.
    pub import_kind: String,
    /// A stored credential this import used, when it used one.
    #[serde(default)]
    pub credential_id: Option<String>,
    pub signals: Vec<IncomingSignalBody>,
}

fn parse_kind(raw: &str) -> ApiResult<SignalKind> {
    match raw.trim() {
        "bookmark" => Ok(SignalKind::Bookmark),
        "kudos" => Ok(SignalKind::Kudos),
        "rating" => Ok(SignalKind::Rating),
        "read" => Ok(SignalKind::Read),
        other => {
            let mut field_errors = std::collections::BTreeMap::new();
            field_errors.insert(
                "kind".to_string(),
                format!("{other:?} is not bookmark, kudos, rating or read"),
            );
            Err(ApiError(darkhold_domain::AppError::Validation {
                message: "unknown signal kind".to_string(),
                field_errors,
            }))
        }
    }
}

async fn import_signals_route(
    State(state): State<AppState>,
    RequirePseud { user, pseud_id }: RequirePseud,
    Json(body): Json<ImportBody>,
) -> ApiResult<Json<Value>> {
    let account = user.account_id.to_string();

    for (field, value) in [
        ("source_key", body.source_key.trim()),
        ("import_kind", body.import_kind.trim()),
    ] {
        if value.is_empty() {
            return Err(ApiError(darkhold_domain::AppError::field(
                field,
                "must not be empty",
            )));
        }
    }
    if body.signals.is_empty() {
        // The store refuses this too; refusing here names the field, and an import with no signals is a client bug rather
        // than an empty history.
        return Err(ApiError(darkhold_domain::AppError::field(
            "signals",
            "an import carries at least one signal",
        )));
    }
    if body.signals.len() > MAX_SIGNALS_PER_IMPORT {
        return Err(ApiError(darkhold_domain::AppError::field(
            "signals",
            &format!("at most {MAX_SIGNALS_PER_IMPORT} in one import"),
        )));
    }

    // A credential id from the body is checked against the caller's own pseud before it is attached to a run row.
    if let Some(credential_id) = body.credential_id.as_deref() {
        let owned = darkhold_db::imports::get_source_credential(state.db(), credential_id)
            .await
            .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?
            .filter(|row| row.pseud_id == pseud_id.to_string())
            .ok_or_else(|| {
                ApiError(darkhold_domain::AppError::field(
                    "credential_id",
                    "not a credential of yours",
                ))
            })?;
        if owned.source_key != body.source_key.trim() {
            return Err(ApiError(darkhold_domain::AppError::field(
                "credential_id",
                &format!(
                    "that credential belongs to {}, and this import is from {}",
                    owned.source_key,
                    body.source_key.trim()
                ),
            )));
        }
    }

    let mut signals = Vec::with_capacity(body.signals.len());
    for incoming in &body.signals {
        if incoming.source_signal_key.trim().is_empty() {
            return Err(ApiError(darkhold_domain::AppError::field(
                "source_signal_key",
                "the source's own id for the item, which the import's idempotency is keyed on",
            )));
        }
        if incoming.dimension_key.trim().is_empty() {
            return Err(ApiError(darkhold_domain::AppError::field(
                "dimension_key",
                "which tag dimension this signal feeds",
            )));
        }
        signals.push(IncomingSignal {
            kind: parse_kind(&incoming.kind)?,
            dimension_key: incoming.dimension_key.trim().to_string(),
            source_signal_key: incoming.source_signal_key.trim().to_string(),
            occurred_at: incoming.occurred_at.clone(),
            provenance_json: incoming.provenance_json.clone(),
        });
    }

    let outcome = taste_import::import_signals(
        state.db(),
        &account,
        body.source_key.trim(),
        body.credential_id.as_deref(),
        body.import_kind.trim(),
        &signals,
    )
    .await
    .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;

    Ok(Json(json!({
        "import_id": outcome.import_id,
        "seen": outcome.seen,
        "added": outcome.added,
    })))
}

/// The split between what the reader gave this instance and what was imported, in rows and in weight.
///
/// §49.6 discounts an imported signal, and a report that showed only the row counts would hide exactly the number the
/// discount exists to move: twenty imported bookmarks and ten ratings given today are thirty rows and twenty-five units
/// of evidence. The counts come from `origin_split`, the weights from the rows it counts — both, because "how many" and
/// "how much" are different questions with different answers.
async fn origin_split_route(
    State(state): State<AppState>,
    MaybeSession(user): MaybeSession,
) -> ApiResult<Json<Value>> {
    let account = account_of(user)?;
    let split = taste_import::origin_split(state.db(), &account)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;
    let signals = taste_import::signals_by_origin(state.db(), &account)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;

    let organic_weight: f64 = signals
        .iter()
        .filter(|s| s.origin == taste_import::SignalOrigin::Organic)
        .map(|s| s.effective_value)
        .sum();
    let imported_weight: f64 = signals
        .iter()
        .filter(|s| s.origin == taste_import::SignalOrigin::Imported)
        .map(|s| s.effective_value)
        .sum();

    Ok(Json(json!({
        "organic": split.organic,
        "imported": split.imported,
        "organic_weight": organic_weight,
        "imported_weight": imported_weight,
        // Named so the number is explainable without reading the crate: §49.6 fixes the direction of the discount and
        // leaves the magnitude to us, and this is the magnitude.
        "imported_discount": taste_import::IMPORTED_SIGNAL_DISCOUNT,
    })))
}

/// The audit row of one import, for the reader who ran it.
async fn import_run_route(
    State(state): State<AppState>,
    MaybeSession(user): MaybeSession,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let account = account_of(user)?;
    let run = taste_import::import_run(state.db(), &id)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;
    match run {
        Some(run) if run.account_id == account => Ok(Json(json!({
            "id": run.id,
            "source_key": run.source_key,
            "credential_id": run.credential_id,
            "import_kind": run.import_kind,
            "signals_seen": run.signals_seen,
            "signals_added": run.signals_added,
            "started_at": run.started_at,
            "finished_at": run.finished_at,
        }))),
        _ => Err(ApiError(darkhold_domain::AppError::NotFound {
            resource: "import",
        })),
    }
}
