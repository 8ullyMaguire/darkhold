//! §52.1 — the operator's taste-leakage view, at `/admin/discovery`.
//!
//! Exists because §52.2 and §52.3 are unreachable without it: the batch windows
//! and owner-visible labels are stored, but an operator cannot see the artifacts
//! they exist to contain.
//!
//! Two decisions worth stating, both of which the spec's own prose forced rather
//! than a house convention:
//!
//!   * **A non-operator gets `NotFound`, not 403.** Following
//!     `decision_service::require_operator`: a 403 confirms the endpoint exists,
//!     and for this view the existence IS a disclosure — a reader probing
//!     `/admin/discovery` learns the operator runs a leakage review at all, which
//!     is one of the things the review is about.
//!   * **The response carries no counts.** §52.1: "a public 'we hide N things'
//!     count would itself become a probe: the difference between the count now and
//!     after a configuration change is a measurement of the operator's taste."
//!     There is no total, no per-ease histogram, and no completeness claim.

use axum::extract::State;
use axum::routing::get;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::RequireSession;
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

use darkhold_db::taste_leakage as tl;
use darkhold_domain::error::AppError;
use darkhold_domain::leakage::{Disposition, Ease, LeakageReview, LeakageRow};

/// Refuse a caller who is not this instance's operator, without confirming that
/// the view exists.
///
/// The comment on `decision_service::require_operator` says it better and it is
/// the same reasoning: an endpoint whose existence is a disclosure must answer 404,
/// because 403 says "yes, and you may not".
fn require_operator(state: &AppState, user: &crate::auth::SessionUser) -> ApiResult<()> {
    // One decision, one place (ADR 0003): fourteen copies of this predicate, all keyed on a config value that is
    // unset on this host, meant every one of these doors answered 404 to everybody.
    crate::routes::admin_scope::require_estate_hidden(state, user)
}

/// What the caller may filter by. No pagination parameter on purpose.
///
/// §52.1's row list is a review queue, and a queue that can be paged is a queue
/// whose *size* can be measured. The store reads every row the review contains;
/// there is no page to walk.
#[derive(Debug, Default, Deserialize)]
pub struct LeakageQuery {
    /// Return only rows with this disposition.
    pub disposition: Option<String>,
}

/// `GET /admin/discovery/leakage` — what an observant user could infer.
pub async fn get_leakage(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    axum::extract::Query(query): axum::extract::Query<LeakageQuery>,
) -> ApiResult<Json<Value>> {
    require_operator(&state, &user)?;

    let review = load_review(&state, query.disposition.as_deref()).await?;

    // Only rows whose wording is prose can be displayed. A row carrying a number
    // is not shown in a "withheld" list either -- naming the withheld rows would
    // publish their contents, which is the disclosure the CHECK was preventing.
    let rows: Vec<Value> = review
        .displayable()
        .into_iter()
        .map(|r| {
            json!({
                "artifact": r.artifact,
                "inferable": r.inferable,
                "ease": ease_str(r.ease),
                "disposition": r.disposition.as_str(),
                "reviewed_at": r.reviewed_at,
            })
        })
        .collect();

    Ok(Json(json!({
        "rows": rows,
        "reviewed_at": review.reviewed_at,
        // §52.1: the view never certifies completeness, and the response says so
        // rather than leaving it to be assumed either way.
        "exhaustive": false,
        "note": "Reviewed artifacts only. Absence of a row is not a claim that \
                 nothing else leaks.",
    })))
}


/// `POST /admin/discovery/leakage` — record one reviewed artifact.
///
/// The view existed and nothing could write a row for it: `record_review` had no caller in `crates/app`, so the queue was
/// permanently empty and read as "nothing leaks here" when it meant "nobody has looked". This is the producer.
///
/// The reviewer is the operator's **pseud**, not the account, because 0110 declares the column that way and §52.1's audit
/// question is "which identity judged this" — the same identity whose taste the row is about. `RequirePseud` refuses a session
/// with no pseud selected, which is the honest answer: a review written by an account acting as nobody has no reviewer.
///
/// The disposition is validated by name through `Disposition::parse` (the same function the GET's filter uses, so the two
/// spellings cannot drift) and `ease` through the same `parse_ease` the view renders with. A refused value names the field,
/// because a typo silently landing as `keep` would be an operator certifying an artifact they meant to coarsen.
#[derive(Debug, Deserialize)]
pub struct ReviewBody {
    pub artifact: String,
    pub inferable: String,
    pub ease: String,
    pub disposition: String,
}

pub async fn post_leakage(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Json(body): Json<ReviewBody>,
) -> ApiResult<Json<Value>> {
    require_operator(&state, &user)?;

    // The pseud comes from the session rather than an extractor, so the audience stays `Operator` (this file's door) and a
    // session that has not selected one is told which field is missing instead of getting the 404 an extractor refusal gives.
    // `route_inventory` is the reason this is not `RequirePseud`: the audience table models who may call, and this door is
    // Operator — the pseud is a requirement *of* the request, not a different audience.
    let Some(pseud_id) = user.pseud_id else {
        let mut field_errors = std::collections::BTreeMap::new();
        field_errors.insert(
            "pseud".to_owned(),
            "select a pseud before recording a review; 0110 names the pseud as the reviewer".to_owned(),
        );
        return Err(ApiError(AppError::Validation {
            message: "a review needs a reviewer identity".to_owned(),
            field_errors,
        }));
    };

    let Some(disposition) = Disposition::parse(&body.disposition) else {
        let mut field_errors = std::collections::BTreeMap::new();
        field_errors.insert(
            "disposition".to_owned(),
            "must be keep, coarsen, or remove".to_owned(),
        );
        return Err(ApiError(AppError::Validation {
            message: "the leakage review takes a disposition from the schema's vocabulary".to_owned(),
            field_errors,
        }));
    };
    if !matches!(body.ease.as_str(), "plain" | "derived" | "measured") {
        let mut field_errors = std::collections::BTreeMap::new();
        field_errors.insert(
            "ease".to_owned(),
            "must be plain, derived, or measured".to_owned(),
        );
        return Err(ApiError(AppError::Validation {
            message: "the leakage review takes an ease from the schema's vocabulary".to_owned(),
            field_errors,
        }));
    }
    if body.artifact.trim().is_empty() || body.inferable.trim().is_empty() {
        let mut field_errors = std::collections::BTreeMap::new();
        if body.artifact.trim().is_empty() {
            field_errors.insert("artifact".to_owned(), "must not be empty".to_owned());
        }
        if body.inferable.trim().is_empty() {
            field_errors.insert("inferable".to_owned(), "must not be empty".to_owned());
        }
        return Err(ApiError(AppError::Validation {
            message: "a review row is a finding in words, so both halves are required".to_owned(),
            field_errors,
        }));
    }

    let reviewed_at = time::OffsetDateTime::now_utc().unix_timestamp();
    let id = tl::record_review(
        state.db(),
        &body.artifact,
        &body.inferable,
        &body.ease,
        disposition,
        pseud_id,
        reviewed_at,
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e)))?;

    // No count, no total: §52.1's rule applies to the write's answer as much as to the view's, and a response that said
    // "review 15" would publish the queue's size one row at a time.
    Ok(Json(json!({ "id": id.to_string(), "reviewed_at": reviewed_at })))
}

fn ease_str(ease: Ease) -> &'static str {
    match ease {
        Ease::Plain => "plain",
        Ease::Derived => "derived",
        Ease::Measured => "measured",
    }
}

/// Read the review.
///
/// The rows are read from the review table rather than recomputed from the taste
/// vector on demand: §52.4 says nothing here makes the taste vector readable, and
/// a view that derived its own rows would be a second implementation of a rule
/// that 0110 already enforces.
async fn load_review(
    state: &AppState,
    disposition_filter: Option<&str>,
) -> ApiResult<LeakageReview> {
    if let Some(raw) = disposition_filter {
        if Disposition::parse(raw).is_none() {
            let mut field_errors = std::collections::BTreeMap::new();
            field_errors.insert(
                "disposition".to_owned(),
                "must be keep, coarsen, or remove".to_owned(),
            );
            return Err(ApiError(AppError::Validation {
                message: "the leakage view takes an optional disposition filter".to_owned(),
                field_errors,
            }));
        }
    }
    let parsed_filter = disposition_filter
        .map(|raw| Disposition::parse(raw).expect("validated above, so this parse cannot fail"));
    let stored = tl::review_rows(state.db(), parsed_filter)
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?;

    let rows: Vec<LeakageRow> = stored
        .into_iter()
        .map(
            |(artifact, inferable, ease, disposition, reviewed_by, reviewed_at)| LeakageRow {
                artifact,
                inferable,
                ease: parse_ease(&ease),
                disposition,
                reviewed_by,
                reviewed_at,
            },
        )
        .collect();

    // `reviewed_at` on the review is the newest row's time, and is 0 when there
    // are no rows -- a review that has not happened has no date, which is more
    // honest than inventing one.
    let reviewed_at = rows.iter().map(|r| r.reviewed_at).max().unwrap_or(0);
    Ok(LeakageReview::new(rows, reviewed_at))
}

fn parse_ease(raw: &str) -> Ease {
    match raw {
        "plain" => Ease::Plain,
        "derived" => Ease::Derived,
        "measured" => Ease::Measured,
        other => panic!("a stored ease is one of the three, enforced by CHECK: {other}"),
    }
}

pub fn router() -> axum::Router<AppState> {
    axum::Router::new().route(
        "/admin/discovery/leakage",
        get(get_leakage).post(post_leakage),
    )
}
