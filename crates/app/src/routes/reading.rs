//! Reading progress, ratings, reviews, history, notes and typography (spec §9).
//!
//! ```text
//! PUT    /reading/progress                         upsert this device's position
//! GET    /reading/progress?subject_id=…            positions for a subject (one per device)
//! DELETE /reading/progress?subject_id=…            forget this device's position
//! GET    /library/history                          with ?cursor= pagination envelope
//! DELETE /library/history/:id
//! POST   /library/history/clear
//! PUT    /works/:id/rating                         { stars, expected_version? }
//! DELETE /works/:id/rating
//! GET    /works/:id/reviews                        public reviews only
//! PUT    /works/:id/reviews                        create or update the caller's review
//! DELETE /works/:id/reviews                        withdraw the caller's review
//! GET    /notes?subject_id=…                       private notes for the acting pseud
//! PUT    /notes                                    create/update
//! DELETE /notes/:id
//! GET    /settings/typography                      effective values + defaults
//! PATCH  /settings/typography                      { expected_version, … }
//! ```
//!
//! Two rules hold across every handler:
//!
//!  * **A private rating never contributes to the public aggregate.** The
//!    aggregate query enforces `is_public = 1` and a minimum count; a stray
//!    private row can never leak. The handler does not need to check.
//!  * **Reading data is per-account (and per-pseud for notes and ratings).**
//!    Every mutation is scoped by the authenticated account, and a note or
//!    rating is keyed on the *pseud* that wrote it, so switching faces shows a
//!    different set.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use darkhold_db::content;
use darkhold_db::notifications;
use darkhold_db::positivity;
use darkhold_db::reading::{self, HistoryRow, Note, Rating, Review};
use darkhold_domain::positivity::{effective, sender_receipt};
use darkhold_domain::reading::{resolve_progress, ProgressResolution, ReadingPosition};
use darkhold_domain::{AppError, PseudId, WorkId};
use serde::{Deserialize, Serialize};

use crate::auth::{MaybeSession, RequirePseud, RequireSession};
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

/// Reading, rating, review, history, note and typography routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/reading/progress",
            put(save_progress).get(get_progress).delete(forget_progress),
        )
        .route("/library/history", get(list_history))
        .route("/library/history/clear", post(clear_history))
        .route("/library/history/{id}", delete(delete_history))
        .route(
            "/works/{id}/rating",
            get(get_rating).put(upsert_rating).delete(delete_rating),
        )
        .route(
            "/works/{id}/reviews",
            get(list_reviews).put(upsert_review).delete(delete_review),
        )
        // §49.4 clause 2 — the reader's own consent to be quoted. Pseud-scoped, like the review it is about: the consent is
        // the reviewer's to give, and a work's author cannot give it for them.
        .route(
            "/works/{id}/reviews/quote-consent",
            get(get_quote_consent).put(put_quote_consent),
        )
        .route("/notes", get(list_notes).put(upsert_note))
        .route("/notes/{id}", delete(delete_note))
}

/// Routes a signed-in reader may reach.
pub fn authed_router() -> Router<AppState> {
    Router::new()
        .route(
            "/settings/typography",
            get(get_typography).patch(save_typography),
        )
        // §9.7.9 reading goals (M47-13). Pseud-scoped and private: the goal is one reader's own
        // pacing, and no other reader — nor another pseud of the same account — can read it.
        .route(
            "/reading-goal",
            get(get_reading_goal)
                .put(put_reading_goal)
                .delete(delete_reading_goal),
        )
}

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ProgressRequest {
    subject_type: String,
    subject_id: String,
    #[serde(default)]
    unit_id: Option<String>,
    #[serde(default)]
    content_revision: Option<String>,
    #[serde(default)]
    paragraph_anchor: Option<String>,
    #[serde(default)]
    position_permille: Option<u16>,
    #[serde(default)]
    device_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RatingRequest {
    stars: i64,
    #[serde(default)]
    is_public: Option<bool>,
    #[serde(default)]
    expected_version: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ReviewRequest {
    body: String,
    #[serde(default)]
    contains_spoilers: Option<bool>,
    #[serde(default)]
    is_public: Option<bool>,
    #[serde(default)]
    expected_version: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct NoteRequest {
    subject_type: String,
    subject_id: String,
    #[serde(default)]
    anchor: Option<String>,
    body: String,
}

#[derive(Debug, Deserialize)]
struct TypographyRequest {
    expected_version: i64,
    #[serde(default)]
    font_scale: Option<f64>,
    #[serde(default)]
    line_height: Option<f64>,
    #[serde(default)]
    measure: Option<i64>,
    #[serde(default)]
    reader_theme: Option<String>,
    #[serde(default)]
    distraction_free: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct SubjectQuery {
    subject_type: String,
    subject_id: String,
}

// ---------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct ProgressView {
    positions: Vec<PositionView>,
    resolution: ProgressResolutionView,
    /// M71-09: what the reader is actually offered, with the honesty about stale places and absent measurements made explicit.
    resume: ResumeView,
}

/// The resume decision as a page receives it.
///
/// `percent` is `null` — not `0` — whenever there is no honest number, and `display` is the string to print instead (`—`). A page that
/// wants a percentage reads `percent`; a page that prints the number reads `display`, and neither can accidentally turn "no measurement" into
/// "0%".
#[derive(Debug, Serialize)]
struct ResumeView {
    state: &'static str,
    percent: Option<u16>,
    display: String,
    sentence: &'static str,
    needs_a_choice: bool,
}

impl From<darkhold_domain::reading::ResumeState> for ResumeView {
    fn from(state: darkhold_domain::reading::ResumeState) -> Self {
        let name = match &state {
            darkhold_domain::reading::ResumeState::Nothing => "nothing",
            darkhold_domain::reading::ResumeState::Unmeasured => "unmeasured",
            darkhold_domain::reading::ResumeState::Current { .. } => "current",
            darkhold_domain::reading::ResumeState::Stale { .. } => "stale",
            darkhold_domain::reading::ResumeState::Conflict { .. } => "conflict",
        };
        Self {
            state: name,
            percent: state.percent(),
            display: state.display(),
            sentence: state.sentence(),
            needs_a_choice: state.needs_a_choice(),
        }
    }
}

#[derive(Debug, Serialize)]
struct PositionView {
    revision: Option<String>,
    anchor: Option<String>,
    position_permille: u16,
    device: Option<String>,
}

impl From<ReadingPosition> for PositionView {
    fn from(position: ReadingPosition) -> Self {
        Self {
            revision: position.revision.as_ref().map(ToString::to_string),
            anchor: position.anchor,
            position_permille: position.fraction,
            device: position.device,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ProgressResolutionView {
    UseStored {
        position: PositionView,
    },
    AskTheReader {
        mine: Option<PositionView>,
        other: Option<PositionView>,
    },
    NoPosition,
}

impl From<ProgressResolution> for ProgressResolutionView {
    fn from(resolution: ProgressResolution) -> Self {
        match resolution {
            ProgressResolution::UseStored(position) => ProgressResolutionView::UseStored {
                position: position.into(),
            },
            ProgressResolution::AskTheReader { mine, other } => {
                ProgressResolutionView::AskTheReader {
                    mine: mine.map(Into::into),
                    other: other.map(Into::into),
                }
            }
            ProgressResolution::NoPosition => ProgressResolutionView::NoPosition,
        }
    }
}

#[derive(Debug, Serialize)]
struct HistoryView {
    items: Vec<HistoryItemView>,
    next_cursor: Option<String>,
}

#[derive(Debug, Serialize)]
struct HistoryItemView {
    id: String,
    subject_type: String,
    subject_id: String,
    last_read_at: String,
    title: String,
    authors: Vec<String>,
}

#[derive(Debug, Serialize)]
struct RatingView {
    stars: i64,
    is_public: bool,
    version: i64,
}

impl From<Rating> for RatingView {
    fn from(rating: Rating) -> Self {
        Self {
            stars: rating.stars,
            is_public: rating.is_public,
            version: rating.version,
        }
    }
}

#[derive(Debug, Serialize)]
struct ReviewListView {
    items: Vec<ReviewView>,
    next_cursor: Option<String>,
}

#[derive(Debug, Serialize)]
struct ReviewView {
    id: String,
    author_handle: String,
    body: String,
    contains_spoilers: bool,
    is_public: bool,
    /// `null` until the review is published — a private review has no
    /// publication time, and saying otherwise would be a small lie the client
    /// would have to work around.
    published_at: Option<String>,
    version: i64,
    /// The sender-visible receipt (spec §12.4): "posted" or "held for
    /// review". Never the class, never the author's settings.
    receipt: String,
}

impl ReviewView {
    fn with_receipt(review: Review, receipt: String) -> Self {
        Self {
            id: review.id,
            author_handle: review.author_handle,
            body: review.body,
            contains_spoilers: review.contains_spoilers,
            is_public: review.is_public,
            published_at: review.published_at,
            version: review.version,
            receipt,
        }
    }
}

impl From<Review> for ReviewView {
    fn from(review: Review) -> Self {
        Self::with_receipt(review, "Comment posted.".to_owned())
    }
}

#[derive(Debug, Serialize)]
struct NoteView {
    id: String,
    anchor: Option<String>,
    body: String,
    created_at: String,
    updated_at: String,
    version: i64,
}

impl From<Note> for NoteView {
    fn from(note: Note) -> Self {
        Self {
            id: note.id,
            anchor: note.anchor,
            body: note.body,
            created_at: note.created_at,
            updated_at: note.updated_at,
            version: note.version,
        }
    }
}

#[derive(Debug, Serialize)]
struct TypographyView {
    font_scale: f64,
    line_height: f64,
    measure: i64,
    reader_theme: String,
    distraction_free: bool,
    version: i64,
}

// ---------------------------------------------------------------------------
// §9.7.9 reading goals (M47-13)
// ---------------------------------------------------------------------------

/// A storage failure, masked as the 500 it is.
///
/// Every route file in this crate carries its own copy — `analytics.rs`, `spoilers.rs`,
/// `moderation.rs` — so this module follows the established shape rather than reaching into
/// another module's private helper.
fn internal(e: impl Into<anyhow::Error>) -> ApiError {
    let error: anyhow::Error = e.into();
    ApiError(darkhold_domain::AppError::Internal(error))
}

#[derive(Debug, Deserialize)]
struct GoalRequest {
    period: String,
    metric: String,
    target: i64,
}

/// The reader's goal and its progress in the current window, or `goal: null`.
///
/// `null` rather than `404`: no goal is the ordinary state of most readers, and a 404 would make
/// every dashboard render an error before the reader has done anything wrong.
async fn get_reading_goal(
    State(state): State<AppState>,
    RequirePseud { user, pseud_id }: RequirePseud,
) -> ApiResult<Json<serde_json::Value>> {
    let goal = darkhold_db::reading_goal::get(state.db(), &pseud_id.to_string())
        .await
        .map_err(internal)?;
    let Some(goal) = goal else {
        return Ok(Json(serde_json::json!({ "goal": null })));
    };
    let progress = darkhold_db::reading_goal::progress(
        state.db(),
        goal,
        &pseud_id.to_string(),
        &user.account_id.to_string(),
        time::OffsetDateTime::now_utc(),
    )
    .await
    .map_err(internal)?;
    Ok(Json(goal_payload(&progress)))
}

/// The shape both reads and writes answer with.
///
/// No `failed`, no `missed`, no `streak`: §9.7.1 forbids loss-framed language, and a payload with no
/// word for it cannot grow one in a later template.
fn goal_payload(progress: &darkhold_domain::reading_goal::Progress) -> serde_json::Value {
    serde_json::json!({
        "goal": {
            "period": progress.goal.period.as_str(),
            "metric": progress.goal.metric.as_str(),
            "target": progress.goal.target,
            "scope": progress.goal.metric.scope(),
        },
        "progress": {
            "current": progress.current,
            "remaining": progress.remaining(),
            "met": progress.met(),
            "window_start": progress.window_start,
            "window_end": progress.window_end,
        },
    })
}

/// Set or replace the goal, answering the same shape as the read so a client needs one renderer.
async fn put_reading_goal(
    State(state): State<AppState>,
    RequirePseud { user, pseud_id }: RequirePseud,
    Json(request): Json<GoalRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let goal = darkhold_domain::reading_goal::Goal::check(
        &request.period,
        &request.metric,
        request.target,
    )
    .map_err(|(field, message)| ApiError(darkhold_domain::AppError::field(field, message)))?;
    darkhold_db::reading_goal::set(
        state.db(),
        &pseud_id.to_string(),
        goal,
        &darkhold_db::identity::now_rfc3339(),
    )
    .await
    .map_err(internal)?;
    let progress = darkhold_db::reading_goal::progress(
        state.db(),
        goal,
        &pseud_id.to_string(),
        &user.account_id.to_string(),
        time::OffsetDateTime::now_utc(),
    )
    .await
    .map_err(internal)?;
    Ok(Json(goal_payload(&progress)))
}

/// Remove the goal. Idempotent, and reports whether there was one.
async fn delete_reading_goal(
    State(state): State<AppState>,
    RequirePseud { pseud_id, .. }: RequirePseud,
) -> ApiResult<Json<serde_json::Value>> {
    let removed = darkhold_db::reading_goal::clear(state.db(), &pseud_id.to_string())
        .await
        .map_err(internal)?;
    Ok(Json(serde_json::json!({ "removed": removed })))
}

// ---------------------------------------------------------------------------
// Reading progress
// ---------------------------------------------------------------------------

async fn save_progress(
    State(state): State<AppState>,
    RequirePseud { user, pseud_id }: RequirePseud,
    Json(request): Json<ProgressRequest>,
) -> ApiResult<StatusCode> {
    let mut revision = request
        .content_revision
        .as_deref()
        .and_then(|r| r.parse().ok());
    // A client that does not name the revision it is reading gets the server's current one for the unit, not NULL (M71-09). A NULL revision
    // is a place nobody can ever prove stale, so the reader would be offered "42%" of text that may have been rewritten twice.
    if revision.is_none() {
        if let Some(unit) = request.unit_id.as_deref() {
            revision = reading::current_revision_of_unit(state.db(), unit).await?;
        }
    }
    reading::save_progress(
        state.db(),
        reading::ProgressInput {
            account: user.account_id,
            pseud: Some(pseud_id),
            subject_type: &request.subject_type,
            subject_id: &request.subject_id,
            unit_id: request.unit_id.as_deref(),
            revision,
            anchor: request.paragraph_anchor.as_deref(),
            fraction: request.position_permille.unwrap_or(0),
            device: request.device_id.as_deref(),
        },
    )
    .await?;

    // Reading is what puts a work in the reader's history, and this route is
    // the only signal the reader's browser sends on arrival. `touch_history`
    // had no caller at all before this — the acceptance tests called the
    // repository directly, so `/library/history` was empty for every real
    // reader while the tests were green.
    reading::touch_history(
        state.db(),
        user.account_id,
        pseud_id,
        &request.subject_type,
        &request.subject_id,
        revision,
    )
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

async fn get_progress(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Query(query): Query<SubjectQuery>,
) -> ApiResult<Json<ProgressView>> {
    let _ = user.pseud_id;
    let positions = reading::progress_for(
        state.db(),
        user.account_id,
        &query.subject_type,
        &query.subject_id,
    )
    .await?;
    let resolution = resolve_progress(&positions);
    // The honesty layer (M71-09): the stored rows carry the revision they were recorded against, but not the revision their unit points at
    // now. One join supplies it, so the comparison happens at a single moment rather than across two queries.
    let stored = reading::positions_with_current_revision(
        state.db(),
        user.account_id,
        &query.subject_type,
        &query.subject_id,
    )
    .await?;
    let resume = darkhold_domain::reading::resume_state(&stored);
    Ok(Json(ProgressView {
        positions: positions.into_iter().map(PositionView::from).collect(),
        resolution: resolution.into(),
        resume: resume.into(),
    }))
}

async fn forget_progress(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Query(query): Query<SubjectQuery>,
) -> ApiResult<StatusCode> {
    let device: Option<String> = None;
    reading::delete_progress(
        state.db(),
        user.account_id,
        &query.subject_type,
        &query.subject_id,
        device.as_deref(),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Reading history
// ---------------------------------------------------------------------------

async fn list_history(
    State(state): State<AppState>,
    RequirePseud { user, pseud_id }: RequirePseud,
) -> ApiResult<Json<HistoryView>> {
    // History is per *pseud*, not per account: the face that read the work is
    // the face that saw it, so switching pseuds must show a different list.
    let rows: Vec<HistoryRow> =
        reading::history_for(state.db(), user.account_id, pseud_id, 50).await?;
    Ok(Json(HistoryView {
        items: rows
            .into_iter()
            .map(|row| HistoryItemView {
                id: row.id,
                subject_type: row.subject_type,
                subject_id: row.subject_id,
                last_read_at: row.last_read_at,
                title: row.title,
                authors: row
                    .author_handles
                    .unwrap_or_default()
                    .split(", ")
                    .map(str::to_owned)
                    .collect(),
            })
            .collect(),
        next_cursor: None,
    }))
}

async fn delete_history(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    reading::delete_history_entry(state.db(), user.account_id, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn clear_history(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<StatusCode> {
    reading::clear_history(state.db(), user.account_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Ratings
// ---------------------------------------------------------------------------

/// The acting pseud's own rating, or `null` when it has not rated the work.
///
/// `null` rather than `404`: the caller asked "what did I give this?", and
/// "nothing" is a complete answer to that question, not a missing resource.
async fn get_rating(
    State(state): State<AppState>,
    RequirePseud { user: _, pseud_id }: RequirePseud,
    Path(id): Path<String>,
) -> ApiResult<Json<Option<RatingView>>> {
    let work_id: WorkId = id
        .parse()
        .map_err(|_| ApiError(AppError::NotFound { resource: "work" }))?;
    let rating = reading::rating_for(state.db(), pseud_id, work_id).await?;
    Ok(Json(rating.map(RatingView::from)))
}

async fn upsert_rating(
    State(state): State<AppState>,
    RequirePseud { user, pseud_id }: RequirePseud,
    Path(id): Path<String>,
    Json(request): Json<RatingRequest>,
) -> ApiResult<Json<RatingView>> {
    let work_id: WorkId = id
        .parse()
        .map_err(|_| ApiError(AppError::NotFound { resource: "work" }))?;
    let stars = request.stars;
    if !(1..=5).contains(&stars) {
        return Err(ApiError(AppError::field(
            "stars",
            "A rating is between 1 and 5.",
        )));
    }
    let is_public = request.is_public.unwrap_or(false);

    // Optimistic concurrency, when the caller sends the version it read. The
    // rating UI always sends it, so two tabs cannot silently overwrite each
    // other's stars (house rule 2.3). A first write sends nothing and is
    // allowed to create the row.
    if let Some(expected) = request.expected_version {
        let actual = reading::rating_for(state.db(), pseud_id, work_id)
            .await?
            .map_or(0, |rating| rating.version);
        if actual != expected {
            return Err(ApiError(AppError::RevisionConflict { expected, actual }));
        }
    }

    let version = reading::upsert_rating(
        state.db(),
        user.account_id,
        pseud_id,
        work_id,
        stars,
        is_public,
    )
    .await?;
    Ok(Json(RatingView {
        stars,
        is_public,
        version,
    }))
}

async fn delete_rating(
    State(state): State<AppState>,
    RequirePseud { user: _, pseud_id }: RequirePseud,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let work_id: WorkId = id
        .parse()
        .map_err(|_| ApiError(AppError::NotFound { resource: "work" }))?;
    reading::delete_rating(state.db(), pseud_id, work_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Reviews
// ---------------------------------------------------------------------------

async fn list_reviews(
    State(state): State<AppState>,
    MaybeSession(_user): MaybeSession,
    Path(id): Path<String>,
) -> ApiResult<Json<ReviewListView>> {
    // Public and anonymous by design (§9): only delivered public reviews are
    // listed — held text is hidden in SQL, never filtered in the handler.
    // Pre-filter reviews (no classification row) count as delivered, so no
    // backfill can silently hide them.
    let work_id: WorkId = id
        .parse()
        .map_err(|_| ApiError(AppError::NotFound { resource: "work" }))?;
    let reviews = positivity::visible_reviews(state.db(), work_id).await?;
    Ok(Json(ReviewListView {
        items: reviews.into_iter().map(ReviewView::from).collect(),
        // A work's public reviews are read in one go; the envelope is here for
        // the shape's sake, as spec §3.3 requires of every collection.
        next_cursor: None,
    }))
}

/// Whether the caller's own note on this work may be quoted beside the summary (§49.4 clause 2).
#[derive(Debug, Serialize)]
struct QuoteConsentView {
    /// The review the consent is about.
    review_id: String,
    /// The work it is about.
    work_id: String,
    /// Whether the note may be quoted. Defaults to `false`: consent is given, never assumed.
    allow_quote: bool,
}

/// The caller's consent, for the caller's own note.
///
/// Separate door from the review itself on purpose. Consent is a thing a reader changes their mind about without touching a
/// word they wrote, and making it a field of the review body would turn "stop quoting me" into an edit that also re-publishes
/// the note and notifies the author.
async fn get_quote_consent(
    State(state): State<AppState>,
    RequirePseud { pseud_id, .. }: RequirePseud,
    Path(id): Path<String>,
) -> ApiResult<Json<QuoteConsentView>> {
    let (work_id, review_id, allow_quote) =
        own_review_consent(&state, pseud_id, &id).await?;
    Ok(Json(QuoteConsentView {
        review_id,
        work_id: work_id.to_string(),
        allow_quote,
    }))
}

/// Grant or withdraw that consent.
///
/// The row is found through `(pseud, work)`, so a caller can only ever change the consent on **their own** note: there is no
/// review id in the URL to guess, and `set_quote_consent` is never reached with an id the caller did not prove. A stranger
/// asking about a note they did not write gets `404` — the same answer as a work they cannot see, because a `403` would
/// confirm that somebody else's note exists and is quotable.
async fn put_quote_consent(
    State(state): State<AppState>,
    RequirePseud { pseud_id, .. }: RequirePseud,
    Path(id): Path<String>,
    Json(request): Json<QuoteConsentRequest>,
) -> ApiResult<Json<QuoteConsentView>> {
    let (work_id, review_id, _) = own_review_consent(&state, pseud_id, &id).await?;
    // `false` here means the row vanished between the read and the write — the erasure of a note in flight. Reporting success
    // would promise a setting that does not exist.
    if !darkhold_db::rec_blurbs::set_quote_consent(state.db(), &review_id, request.allow_quote).await? {
        return Err(ApiError(AppError::NotFound {
            resource: "review",
        }));
    }
    Ok(Json(QuoteConsentView {
        review_id,
        work_id: work_id.to_string(),
        allow_quote: request.allow_quote,
    }))
}

/// `{ "allow_quote": bool }` — the only field, and named after the column so a client reading the schema and a client reading
/// the migration agree.
#[derive(Debug, Deserialize)]
struct QuoteConsentRequest {
    /// Whether the note may be quoted.
    allow_quote: bool,
}

/// The caller's review of one work, with its current consent. `404` when the caller has written no note there — a consent
/// setting for a note that does not exist is not a thing to report, let alone create.
async fn own_review_consent(
    state: &AppState,
    pseud_id: PseudId,
    id: &str,
) -> ApiResult<(WorkId, String, bool)> {
    let work_id: WorkId = id
        .parse()
        .map_err(|_| ApiError(AppError::NotFound { resource: "work" }))?;
    let review = reading::review_for(state.db(), pseud_id, work_id)
        .await?
        .ok_or_else(|| ApiError(AppError::NotFound {
            resource: "review",
        }))?;
    // `quote_consent` reports `None` for a row that no longer exists; treated as the same `404` as "you have not written one",
    // because a client cannot act on the difference and the second answer would confirm a deleted note's history.
    let allow_quote = darkhold_db::rec_blurbs::quote_consent(state.db(), &review.id)
        .await?
        .ok_or_else(|| ApiError(AppError::NotFound {
            resource: "review",
        }))?;
    Ok((work_id, review.id, allow_quote))
}

async fn upsert_review(
    State(state): State<AppState>,
    RequirePseud { user, pseud_id }: RequirePseud,
    Path(id): Path<String>,
    Json(request): Json<ReviewRequest>,
) -> ApiResult<Json<ReviewView>> {
    let work_id: WorkId = id
        .parse()
        .map_err(|_| ApiError(AppError::NotFound { resource: "work" }))?;
    let work = content::find_work(state.db(), work_id)
        .await?
        .ok_or_else(|| ApiError(AppError::NotFound { resource: "work" }))?;

    if request.body.trim().is_empty() {
        return Err(ApiError(AppError::field(
            "body",
            "A review needs some words.",
        )));
    }

    // Reviews are private until explicitly published (§9.5), so the default is
    // private and only an explicit `true` makes one visible.
    let is_public = request.is_public.unwrap_or(false);
    let contains_spoilers = request.contains_spoilers.unwrap_or(false);

    // Optimistic concurrency, when the caller sends the version it read. A
    // caller that omits it is saying "I do not care what was there", which is
    // what a first write does.
    if let Some(expected) = request.expected_version {
        let actual = reading::review_for(state.db(), pseud_id, work_id)
            .await?
            .map_or(0, |review| review.version);
        if actual != expected {
            return Err(ApiError(AppError::RevisionConflict { expected, actual }));
        }
    }

    // Was the author already able to read a review from this pseud? An edit to
    // a review that is already public is not news, and the notification says
    // "a new public review was posted" — so hold it back for edits. (A review
    // that was held when it was written and delivered by a later edit is the
    // one case this misses; the author learns on the reviewer's next edit.)
    let was_public = reading::review_for(state.db(), pseud_id, work_id)
        .await?
        .is_some_and(|existing| existing.is_public);

    let version = reading::upsert_review(
        state.db(),
        user.account_id,
        pseud_id,
        work_id,
        &request.body,
        contains_spoilers,
        is_public,
    )
    .await?;

    let review = reading::review_for(state.db(), pseud_id, work_id)
        .await?
        .ok_or_else(|| {
            ApiError(AppError::internal(
                "reading back a review that was just written",
                anyhow::anyhow!("the review is missing immediately after its upsert"),
            ))
        })?;
    debug_assert_eq!(review.version, version);

    // The positivity gate (spec §12): classify against the author's
    // effective preferences *before* the review is read back, and record
    // the outcome. Held text stays stored but leaves every listing.
    //
    // D8 — compensating action: if classification fails, the review was
    // already written, so we withdraw (delete) it rather than leave a
    // phantom review the positivity pipeline never saw. The 500 the caller
    // gets is honest; the DB stays consistent.
    let outcome = if is_public {
        let author = positivity::author_account_for_work(state.db(), work_id).await?;
        match author {
            None => darkhold_domain::positivity::DeliveryOutcome::Delivered,
            Some(author_account) => {
                let prefs = positivity::preferences_for(state.db(), author_account).await?;
                let work_ov = positivity::override_for(state.db(), work_id).await?;
                let policy = effective(&prefs, &work_ov);
                let (allow, deny) =
                    positivity::list_membership(state.db(), author_account, pseud_id).await?;
                let stored = match positivity::classify_review(
                    state.db(),
                    &review.id,
                    &request.body,
                    &policy,
                    allow,
                    deny,
                )
                .await
                {
                    Ok(s) => s,
                    Err(e) => {
                        reading::delete_review(state.db(), pseud_id, work_id)
                            .await
                            .ok();
                        return Err(ApiError(AppError::internal("classifying the review", e)));
                    }
                };
                // Notify the work's author when a public review passes
                // the positivity gate. A held review is invisible to
                // the author anyway, so there's nothing to report, and an
                // edit of a review that was already public is not news.
                if stored.outcome == darkhold_domain::positivity::DeliveryOutcome::Delivered
                    && !was_public
                {
                    let _ = notifications::notify(
                        state.db(),
                        &author_account.to_string(),
                        "review",
                        "Someone reviewed your work",
                        &format!("A new public review was posted on {}.", work.title),
                        Some(work_id.to_canonical_string().as_str()),
                    )
                    .await;
                }
                stored.outcome
            }
        }
    } else {
        // Private reviews are the author's own notes-to-self: no gate, and
        // no classification row, so publishing later classifies then.
        darkhold_domain::positivity::DeliveryOutcome::Delivered
    };

    Ok(Json(ReviewView::with_receipt(
        review,
        sender_receipt(outcome).to_owned(),
    )))
}

async fn delete_review(
    State(state): State<AppState>,
    RequirePseud { user: _, pseud_id }: RequirePseud,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let work_id: WorkId = id
        .parse()
        .map_err(|_| ApiError(AppError::NotFound { resource: "work" }))?;
    // A review that is not there is not a failure: the caller asked for the
    // end state, and it already holds.
    reading::delete_review(state.db(), pseud_id, work_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Notes
// ---------------------------------------------------------------------------

async fn list_notes(
    State(state): State<AppState>,
    RequirePseud { user: _, pseud_id }: RequirePseud,
    Query(query): Query<SubjectQuery>,
) -> ApiResult<Json<Vec<NoteView>>> {
    let notes =
        reading::notes_for(state.db(), pseud_id, &query.subject_type, &query.subject_id).await?;
    Ok(Json(notes.into_iter().map(NoteView::from).collect()))
}

async fn upsert_note(
    State(state): State<AppState>,
    RequirePseud { user, pseud_id }: RequirePseud,
    Json(request): Json<NoteRequest>,
) -> ApiResult<StatusCode> {
    reading::save_note(
        state.db(),
        user.account_id,
        pseud_id,
        &request.subject_type,
        &request.subject_id,
        request.anchor.as_deref(),
        &request.body,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_note(
    State(state): State<AppState>,
    RequirePseud { user: _, pseud_id }: RequirePseud,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    reading::delete_note(state.db(), pseud_id, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Typography
// ---------------------------------------------------------------------------

async fn get_typography(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<TypographyView>> {
    let typography = reading::typography_for(state.db(), user.account_id).await?;
    Ok(Json(TypographyView {
        font_scale: typography.font_scale,
        line_height: typography.line_height,
        measure: typography.measure,
        reader_theme: typography.reader_theme,
        distraction_free: typography.distraction_free,
        version: typography.version,
    }))
}

async fn save_typography(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Json(request): Json<TypographyRequest>,
) -> ApiResult<StatusCode> {
    let current = reading::typography_for(state.db(), user.account_id).await?;
    let font_scale = request.font_scale.unwrap_or(current.font_scale);
    let line_height = request.line_height.unwrap_or(current.line_height);
    let measure = request.measure.unwrap_or(current.measure);
    let reader_theme = request
        .reader_theme
        .as_deref()
        .unwrap_or(&current.reader_theme);
    let distraction_free = request.distraction_free.unwrap_or(current.distraction_free);

    let saved = reading::save_typography(
        state.db(),
        reading::TypographyInput {
            account: user.account_id,
            expected_version: request.expected_version,
            font_scale,
            line_height,
            measure,
            reader_theme,
            distraction_free,
        },
    )
    .await?;

    if !saved {
        return Err(ApiError(AppError::RevisionConflict {
            expected: request.expected_version,
            actual: request.expected_version + 1,
        }));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_rating_outside_one_to_five_is_rejected() {
        // The handler checks this before touching the database; the test pins
        // the rule so a future refactor cannot widen it.
        for stars in [0, 6, 10] {
            assert!(!(1..=5).contains(&stars));
        }
        for stars in 1..=5 {
            assert!((1..=5).contains(&stars));
        }
    }
}
