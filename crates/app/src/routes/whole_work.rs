//! M70-02 — whole-work reading: the spans, and where a reader is in them.
//!
//! ```text
//! GET /works/:id/whole                    every chapter's span and the work's length
//! GET /works/:id/whole?at=<chapter>:<n>   the same, plus the reader's place resolved
//! ```
//!
//! # Why the server resolves the position
//!
//! The reader keeps `(chapter, offset)`; the whole-work view scrolls to `start + offset`. Converting between the two
//! is arithmetic on chapter lengths, and doing it in the browser would mean the client and the server disagreed the
//! first time a chapter was added — so the conversion lives in `darkhold_domain::reading_position` and this door
//! exposes it. `?at=` deliberately returns the **canonical** place: a boundary has two spellings, and a client that
//! stores the non-canonical one will ask about it forever.
//!
//! # A missing chapter is a refusal, not a guess
//!
//! `?at=<chapter>` naming a chapter this work does not have returns 422 with the accepted shape. Clamping would put
//! the reader somewhere they never were, silently; moving them is a decision the reader is entitled to notice.

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use darkhold_domain::error::AppError;
use darkhold_domain::reading_position::{canonical, Chapters, Position};

use crate::auth::MaybeSession;
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

/// The whole-work routes.
pub fn router() -> Router<AppState> {
    Router::new().route("/works/{id}/whole", get(whole))
}

#[derive(Debug, Deserialize)]
struct WholeQuery {
    /// `<chapter id>:<offset>` — where the reader is, in the per-chapter frame.
    at: Option<String>,
}

fn invalid(message: impl Into<String>) -> ApiError {
    ApiError(AppError::Validation {
        message: message.into(),
        field_errors: std::collections::BTreeMap::new(),
    })
}

async fn whole(
    State(state): State<AppState>,
    MaybeSession(_session): MaybeSession,
    Path(work): Path<String>,
    Query(query): Query<WholeQuery>,
) -> ApiResult<Json<Value>> {
    let spans = darkhold_db::whole_work::spans(state.db(), &work).await?;
    let lengths: Vec<usize> = spans
        .iter()
        .map(|s| usize::try_from(s.words).unwrap_or(usize::MAX))
        .collect();
    let chapters = Chapters(&lengths);

    let position = match query.at.as_deref() {
        None => Value::Null,
        Some(raw) => {
            let (chapter_id, offset) = raw
                .rsplit_once(':')
                .ok_or_else(|| invalid("at: expected <chapter id>:<offset>"))?;
            let offset: usize = offset.parse().map_err(|_| {
                invalid("at: the offset after the colon must be a whole number of words")
            })?;
            let index = spans
                .iter()
                .position(|span| span.chapter_id == chapter_id)
                .ok_or_else(|| invalid("at: that chapter is not in this work"))?;
            let canonical_position = canonical(
                chapters,
                Position {
                    chapter: index,
                    offset,
                },
            )
            .ok_or_else(|| invalid("at: the offset is past the end of that chapter"))?;
            let span = &spans[canonical_position.chapter];
            json!({
                "chapter": canonical_position.chapter,
                "chapter_id": span.chapter_id,
                "offset": canonical_position.offset,
                "words_before": span.start,
                "words_into_work": span.start + canonical_position.offset as u64,
            })
        }
    };

    let total = darkhold_db::whole_work::total_words(state.db(), &work)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;

    Ok(Json(json!({
        "work_id": work,
        // From the store, not from a second sum over the spans above (M70-13): `whole_work::total_words` is what a test
        // proves, and a page that added the numbers itself could drift from it without either test noticing.
        "total_words": total,
        "chapters": spans
            .iter()
            .enumerate()
            .map(|(index, span)| json!({
                "index": index,
                "chapter_id": span.chapter_id,
                "title": span.title,
                "start": span.start,
                "words": span.words,
            }))
            .collect::<Vec<_>>(),
        "position": position,
    })))
}
