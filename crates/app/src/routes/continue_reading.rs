//! Item 1 of the 100-idea audit: "Continue Reading".
//!
//! Spec: `docs/plans/100-ideas-remaining.md` §2a. Store: `crates/db/src/continue_reading.rs`.
//!
//! # One route, and it is always behind a login
//!
//! `GET /api/continue-reading` answers a question only a signed-in reader can ask: where
//! did **you** stop. There is no anonymous version of that question, so there is no
//! `MaybeSession` here — the reader-surface routes use `MaybeSession` because "new in
//! your fandoms" degrades to "new, unfiltered" for a stranger, and "most bookmarked this
//! week" is public by construction. Neither degradation exists for this one, and serving
//! an empty banner to a stranger is worse than a login wall: it reads as "you have read
//! nothing" when it means "we do not know who you are".
//!
//! # `404` means "nothing to continue", and that is not an error
//!
//! The reader finished everything, or never opened anything. Both are ordinary states,
//! and the banner simply does not render. A `200 {"continue": null}` would be the same
//! information in a shape the client has to unwrap; `404` is what the rest of the API
//! already means by "this does not exist", so the client gets one rule instead of two.
//! `is_not_found` in the frontend client keys on the status, not on the body.

use crate::auth::RequireSession;
use crate::http::ApiResult;
use crate::state::AppState;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/continue-reading", get(continue_reading))
        .route("/library/unfinished", get(unfinished_shelf))
}

/// The wire shape.
///
/// `position_permille` is carried as well as `percent` because they are not redundant.
/// The store returns the raw column so the *rounding* is the caller's decision, and a
/// client that only wants a progress bar should not have to re-derive it — while a client
/// that wants "page 12 of 340" needs the unrounded value. Emitting both means neither
/// side has to guess what the other rounded.
#[derive(Debug, Serialize)]
pub struct ContinueReadingResponse {
    pub work_id: String,
    pub title: String,
    pub position_permille: i32,
    /// Whole percent, already clamped to 0..=100 by the store.
    pub percent: u8,
    pub unit_id: Option<String>,
    pub unit_title: Option<String>,
    /// RFC 3339. When this reader last wrote this row — useful for "you were reading
    /// this three days ago" copy without a second round trip.
    pub updated_at: String,
}

async fn continue_reading(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<ContinueReadingResponse>> {
    let row =
        darkhold_db::continue_reading::continue_reading(state.db(), &user.account_id.to_string())
            .await
            .map_err(|e| crate::http::ApiError(darkhold_domain::AppError::Internal(e)))?
            .ok_or_else(|| {
                // 404, not 200-with-null. See the module comment. The `resource` noun is
                // deliberately coarse and singular: it must not distinguish "you read
                // nothing" from "everything you started is finished", because a signed-in
                // reader can enumerate their own library and does not need the API to
                // summarise it back at them.
                crate::http::ApiError(darkhold_domain::AppError::NotFound {
                    resource: "reading progress",
                })
            })?;

    // Computed before the fields move out. `percent()` borrows `&self`, and the
    // struct-literal below moves `row.work_id` first, so the borrow would be of a
    // partially moved value. Binding it first also keeps the rounding decision visibly
    // the store's, rather than looking like arithmetic invented by the handler.
    let percent = row.percent();

    Ok(Json(ContinueReadingResponse {
        work_id: row.work_id,
        title: row.title,
        position_permille: row.position_permille,
        percent,
        unit_id: row.unit_id,
        unit_title: row.unit_title,
        updated_at: row.updated_at,
    }))
}

// ---------------------------------------------------------------------------
// The unfinished shelf
// ---------------------------------------------------------------------------

/// The default page size, and the ceiling the store clamps to.
///
/// Required and bounded rather than trusted: the store takes a `limit` and clamps it to 1..=50, and the ceiling is reported
/// in the answer so a client can page honestly instead of inferring that a short page means "no more".
const SHELF_DEFAULT_LIMIT: i64 = 20;
const SHELF_MAX_LIMIT: i64 = 50;

#[derive(Debug, serde::Deserialize)]
pub struct ShelfQuery {
    #[serde(default)]
    pub limit: Option<i64>,
}

/// One started work: where the reader stopped, and what they said if they dropped it.
#[derive(Debug, Serialize)]
pub struct ShelfItem {
    pub work_id: String,
    pub title: String,
    pub position_permille: i32,
    pub percent: u8,
    pub unit_id: Option<String>,
    pub unit_title: Option<String>,
    /// RFC 3339. When this reader last moved in this work.
    pub updated_at: String,
    /// `Some` when the reader marked this work did-not-finish, `None` while it is merely in progress. The caller decides
    /// what to do with each reason — that judgement is why the store returns the reason rather than filtering by it.
    pub dnf_reason: Option<String>,
    /// Whether other readers can see that this reader dropped it. `None` when there is no DNF row at all.
    pub dnf_public: Option<bool>,
    /// The reader's own note. Theirs: `dnf_public = false` means nobody else sees this sentence.
    pub dnf_note: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ShelfResponse {
    pub items: Vec<ShelfItem>,
    /// The effective page size after clamping, so a client that asked for 10000 learns what it got.
    pub limit: i64,
}

/// Everything this reader started and has not finished, newest first.
///
/// Two queries that had no caller — `unfinished_works` and `dnf_works_for` — answered the same question in halves: one knew
/// what was started, the other knew what was dropped, and neither knew both. The banner route above deliberately answers
/// only "the one thing you are reading"; this is the shelf behind it.
///
/// A dropped work stays on the shelf with its reason. Dropping a work is not the same as never having started it, and the
/// reason is what lets the caller decide whether to offer it again (`not_my_taste`) or never (`triggering`) — the decision
/// the store refused to make, correctly.
async fn unfinished_shelf(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    axum::extract::Query(query): axum::extract::Query<ShelfQuery>,
) -> ApiResult<Json<ShelfResponse>> {
    let requested = query.limit.unwrap_or(SHELF_DEFAULT_LIMIT);
    let effective = requested.clamp(1, SHELF_MAX_LIMIT);
    let account = user.account_id.to_string();

    let started = darkhold_db::continue_reading::unfinished_works(state.db(), &account, effective)
        .await
        .map_err(|e| crate::http::ApiError(darkhold_domain::AppError::Internal(e)))?;
    let dropped = darkhold_db::continue_reading::dnf_works_for(state.db(), &account)
        .await
        .map_err(|e| crate::http::ApiError(darkhold_domain::AppError::Internal(e)))?;

    // Keyed by work id, and the map is consumed rather than cloned: a work has at most one live DNF row (the store upserts),
    // so a second entry would be a fault worth noticing rather than a value to merge.
    let mut marks: std::collections::HashMap<String, (String, bool, Option<String>)> = dropped
        .into_iter()
        .map(|(work_id, reason, is_public, note)| (work_id, (reason, is_public, note)))
        .collect();

    let items = started
        .into_iter()
        .map(|row| {
            let mark = marks.remove(&row.work_id);
            ShelfItem {
                percent: row.percent(),
                work_id: row.work_id,
                title: row.title,
                position_permille: row.position_permille,
                unit_id: row.unit_id,
                unit_title: row.unit_title,
                updated_at: row.updated_at,
                dnf_reason: mark.as_ref().map(|(reason, _, _)| reason.clone()),
                dnf_public: mark.as_ref().map(|(_, is_public, _)| *is_public),
                dnf_note: mark.and_then(|(_, _, note)| note),
            }
        })
        .collect();

    Ok(Json(ShelfResponse {
        items,
        limit: effective,
    }))
}
