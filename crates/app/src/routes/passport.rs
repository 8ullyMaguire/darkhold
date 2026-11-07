//! M69-03 — the passport doors: take your taste with you, and present one you carried here.
//!
//! * `GET /api/v1/me/passport` — build a passport from **this instance's** evidence and sign it with this instance's key.
//! * `POST /api/v1/me/passport` — present a signed passport from another instance; it is verified, receipted, and its taste
//!   seeded as `imported` evidence.
//! * `GET /api/v1/me/passports` — the receipts this account has presented.
//! * `DELETE /api/v1/me/passport?origin=…` — withdraw one, and the taste it seeded.
//!
//! All four are closed unless `[discovery] taste_passports` is on, and export additionally needs a signing key. A missing key
//! is answered with the reason rather than with an unsigned document: a document that verifies against nothing is worse than
//! no document, because the receiving side would have to decide what to do with it.
//!
//! # What the export contains, and what it deliberately does not
//!
//! Taste (tag weights in basis points), favourite source URLs, contribution **counts**, and the origin's affinity reading.
//! Not the works, not the comments, not the bookmarks: a passport is a starting point, and one that carried a reader's
//! writing to a stranger's server would need a licence the reader never granted. Counts answer "how much have they done"
//! without answering "what did they do".
//!
//! # The signature is over this instance's claim, not the reader's
//!
//! The key is the *instance's*. So a passport proves "this instance said this about this handle" — which is exactly what a
//! receiving instance can act on, and no more. The reader's consent is the act of asking for it.

use std::collections::BTreeMap;

use axum::extract::{Query, State};
use axum::routing::{delete, get};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use darkhold_domain::passport::{self, Contributions, Passport, SignedPassport};

use crate::auth::RequireSession;
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;
use darkhold_domain::error::AppError;

/// The passport routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/me/passport", get(export).post(present))
        .route("/me/passports", get(receipts))
        .route("/me/passport", delete(withdraw))
}

fn closed(state: &AppState) -> Option<ApiError> {
    (!state.config().discovery.taste_passports).then(|| {
        ApiError(AppError::NotFound {
            resource: "taste passport",
        })
    })
}

fn internal(error: anyhow::Error) -> ApiError {
    ApiError(AppError::Internal(error))
}

fn invalid(message: String) -> ApiError {
    ApiError(AppError::Validation {
        message,
        field_errors: BTreeMap::new(),
    })
}

/// This instance's signing key, when one is configured.
fn signing_key_hex(state: &AppState) -> Option<&str> {
    state.config().discovery.passport_key.as_deref()
}

/// The reader's tag weights at this instance, in basis points, from what they chose to collect.
///
/// The shelf is the input for the same reason M64-09 uses it: it is the one collection a reader assembled on purpose, and
/// this schema keeps no per-reader ratings to do better.
async fn taste_bp(state: &AppState, account: &str) -> ApiResult<BTreeMap<String, u32>> {
    let rows: Vec<(String, i64)> = match state.db().backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query_as(
                // The passport carries the tag's canonical name, not the node id it happens to have here: an id is a local
                // key, and a receiving instance reading `7f3a-…` would learn nothing it could compare. The table is
                // `canonical_entities` since 0124 (`taxonomy_nodes` before the rename).
                "SELECT tn.canonical, COUNT(*) AS n
                   FROM bookmarks b
                   JOIN work_tags wt ON wt.work_id = b.subject_id
                   JOIN canonical_entities tn ON tn.id = wt.node_id
                  WHERE b.account_id = ? AND b.subject_type = 'work'
                  GROUP BY tn.canonical
                  ORDER BY n DESC
                  LIMIT 200",
            )
            .bind(account)
            .fetch_all(state.db().sqlite_pool().expect("sqlite"))
            .await
            .map_err(|error| internal(error.into()))?
        }
        darkhold_db::Backend::Postgres => sqlx::query_as(
            "SELECT tn.canonical, CAST(COUNT(*) AS BIGINT) AS n
                   FROM bookmarks b
                   JOIN work_tags wt ON wt.work_id = b.subject_id
                   JOIN canonical_entities tn ON tn.id = wt.node_id
                  WHERE b.account_id = $1 AND b.subject_type = 'work'
                  GROUP BY tn.canonical
                  ORDER BY n DESC
                  LIMIT 200",
        )
        .bind(account)
        .fetch_all(state.db().postgres_pool().expect("postgres"))
        .await
        .map_err(|error| internal(error.into()))?,
    };

    let total: i64 = rows.iter().map(|(_, count)| *count).sum();
    if total <= 0 {
        return Ok(BTreeMap::new());
    }
    Ok(rows
        .into_iter()
        .map(|(node, count)| {
            let share = (count as f64 / total as f64 * 10_000.0).round() as u32;
            (node, share)
        })
        .collect())
}

async fn contribution_counts(state: &AppState, account: &str) -> ApiResult<Contributions> {
    async fn count(state: &AppState, sql_sqlite: &str, sql_pg: &str, bind: &str) -> ApiResult<u32> {
        let value: i64 = match state.db().backend() {
            darkhold_db::Backend::Sqlite => sqlx::query_scalar(sql_sqlite)
                .bind(bind)
                .fetch_one(state.db().sqlite_pool().expect("sqlite"))
                .await
                .map_err(|error| internal(error.into()))?,
            darkhold_db::Backend::Postgres => sqlx::query_scalar(sql_pg)
                .bind(bind)
                .fetch_one(state.db().postgres_pool().expect("postgres"))
                .await
                .map_err(|error| internal(error.into()))?,
        };
        Ok(u32::try_from(value).unwrap_or(u32::MAX))
    }

    let works = count(
        state,
        "SELECT COUNT(*) FROM works w JOIN pseuds p ON p.id = w.owner_pseud_id WHERE p.account_id = ?",
        "SELECT CAST(COUNT(*) AS BIGINT) FROM works w JOIN pseuds p ON p.id = w.owner_pseud_id WHERE p.account_id = $1::uuid",
        account,
    )
    .await?;
    let revisions = count(
        state,
        // `chapter_revisions` is `revisions` since 0122: the lorehaven word for it was renamed away, and a query holding the
        // old name would fail on every live instance while passing on none.
        "SELECT COUNT(*) FROM revisions r JOIN pseuds p ON p.id = r.created_by_pseud_id WHERE p.account_id = ?",
        "SELECT CAST(COUNT(*) AS BIGINT) FROM revisions r JOIN pseuds p ON p.id = r.created_by_pseud_id WHERE p.account_id = $1::uuid",
        account,
    )
    .await?;
    let comments = count(
        state,
        "SELECT COUNT(*) FROM comments c JOIN pseuds p ON p.id = c.author_pseud WHERE p.account_id = ?",
        "SELECT CAST(COUNT(*) AS BIGINT) FROM comments c JOIN pseuds p ON p.id = c.author_pseud WHERE p.account_id = $1::uuid",
        account,
    )
    .await?;

    Ok(Contributions {
        works,
        revisions,
        comments,
    })
}

async fn export(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<Value>> {
    if let Some(error) = closed(&state) {
        return Err(error);
    }
    let Some(key) = signing_key_hex(&state) else {
        return Err(invalid(
            "this instance has no passport signing key configured ([discovery] passport_key), so it cannot vouch for a \
             document — and an unsigned passport is one nobody can check"
                .to_owned(),
        ));
    };

    let account = user.account_id.to_string();
    let taste = taste_bp(&state, &account).await?;
    let contributions = contribution_counts(&state, &account).await?;
    // The store answers with the row it keeps (score, evidence and the three components); the passport carries the score.
    let affinity = darkhold_db::reader_affinity::for_account(state.db(), &account)
        .await
        .map_err(internal)?
        .and_then(|score| score.get("score_bp").and_then(Value::as_i64))
        .and_then(|bp| u16::try_from(bp).ok());

    let handle: Option<String> = match state.db().backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar(
            "SELECT handle FROM pseuds WHERE account_id = ? ORDER BY created_at LIMIT 1",
        )
        .bind(&account)
        .fetch_optional(state.db().sqlite_pool().expect("sqlite"))
        .await
        .map_err(|error| internal(error.into()))?,
        darkhold_db::Backend::Postgres => sqlx::query_scalar(
            "SELECT handle FROM pseuds WHERE account_id = $1::uuid ORDER BY created_at LIMIT 1",
        )
        .bind(&account)
        .fetch_optional(state.db().postgres_pool().expect("postgres"))
        .await
        .map_err(|error| internal(error.into()))?,
    };

    let passport = Passport {
        version: passport::VERSION,
        origin: state
            .config()
            .site
            .base_url
            .trim_end_matches('/')
            .to_owned(),
        subject: handle.unwrap_or_else(|| account.clone()),
        issued_at: darkhold_db::identity::now_rfc3339(),
        taste,
        favourite_sources: favourite_sources(&state, &account).await?,
        contributions,
        affinity_bp: affinity,
    };
    let signed =
        passport::sign_with_hex(&passport, key).map_err(|error| invalid(error.to_string()))?;

    Ok(Json(json!({
        "passport": signed,
        "note": "signed by this instance, not by the reader: a receiving instance treats it as a claim, and its local evidence outvotes it",
    })))
}

/// The source URLs this reader collects from most, deduplicated and capped.
async fn favourite_sources(state: &AppState, account: &str) -> ApiResult<Vec<String>> {
    let rows: Vec<(String,)> = match state.db().backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query_as(
                // `library_items` carries both the work and the URL it came from; there is no separate join table, and a
                // reader's own copies are the ones that say where this person already reads.
                "SELECT li.source_url
                   FROM bookmarks b
                   JOIN library_items li ON li.work_id = b.subject_id AND li.account_id = b.account_id
                  WHERE b.account_id = ? AND b.subject_type = 'work'
                  GROUP BY li.source_url
                  ORDER BY COUNT(*) DESC
                  LIMIT ?",
            )
            .bind(account)
            .bind(i64::try_from(passport::MAX_SOURCES).unwrap_or(100))
            .fetch_all(state.db().sqlite_pool().expect("sqlite"))
            .await
            .map_err(|error| internal(error.into()))?
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query_as(
                // `library_items` carries both the work and the URL it came from; there is no separate join table, and a
                // reader's own copies are the ones that say where this person already reads.
                "SELECT li.source_url
                   FROM bookmarks b
                   JOIN library_items li ON li.work_id = b.subject_id AND li.account_id = b.account_id
                  WHERE b.account_id = $1 AND b.subject_type = 'work'
                  GROUP BY li.source_url
                  ORDER BY COUNT(*) DESC
                  LIMIT ?",
            )
            .bind(account)
            .bind(i64::try_from(passport::MAX_SOURCES).unwrap_or(100))
            .fetch_all(state.db().postgres_pool().expect("postgres"))
            .await
            .map_err(|error| internal(error.into()))?
        }
    };
    Ok(rows.into_iter().map(|(url,)| url).collect())
}

#[derive(Debug, Deserialize)]
struct PresentRequest {
    /// The signed passport, as the origin issued it.
    passport: Value,
}

async fn present(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Json(request): Json<PresentRequest>,
) -> ApiResult<Json<Value>> {
    if let Some(error) = closed(&state) {
        return Err(error);
    }
    let signed: SignedPassport = serde_json::from_value(request.passport)
        .map_err(|error| invalid(format!("not a signed passport: {error}")))?;
    let account = user.account_id.to_string();
    let now = darkhold_db::identity::now_rfc3339();
    let receipt = darkhold_db::passports::import(state.db(), &account, &signed, &now)
        .await
        .map_err(|error| invalid(error.to_string()))?;

    Ok(Json(json!({
        "id": receipt.id,
        "origin": receipt.origin,
        "subject": receipt.subject,
        "issued_at": receipt.issued_at,
        "signals": receipt.signals,
        "taste_bp": receipt.taste_bp,
        "note": "recorded as imported evidence: your own activity here outvotes it as it accumulates",
    })))
}

async fn receipts(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<Value>> {
    if let Some(error) = closed(&state) {
        return Err(error);
    }
    let rows = darkhold_db::passports::for_account(state.db(), &user.account_id.to_string())
        .await
        .map_err(internal)?;
    Ok(Json(json!({
        "passports": rows.iter().map(|row| json!({
            "id": row.id,
            "origin": row.origin,
            "key": row.key,
            "issued_at": row.issued_at,
            "received_at": row.received_at,
            "signals": row.signals,
        })).collect::<Vec<Value>>(),
    })))
}

#[derive(Debug, Deserialize)]
struct WithdrawQuery {
    origin: String,
}

async fn withdraw(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Query(query): Query<WithdrawQuery>,
) -> ApiResult<Json<Value>> {
    if let Some(error) = closed(&state) {
        return Err(error);
    }
    let removed = darkhold_db::passports::revoke(
        state.db(),
        &user.account_id.to_string(),
        query.origin.trim(),
    )
    .await
    .map_err(internal)?;
    if !removed {
        return Err(ApiError(AppError::NotFound {
            resource: "passport from that origin",
        }));
    }
    Ok(Json(json!({
        "withdrawn": query.origin,
        "note": "the receipt and the taste it seeded are both gone: evidence with no provenance is not evidence",
    })))
}
