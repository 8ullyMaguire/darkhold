//! §13's account archive over HTTP (`M45-39` phase 2).
//!
//! `GET /api/v1/me/archive` answers one document: the account, its pseuds, its works, its shelf, its
//! reading state, its ratings and reviews, and its credit balances, each with a SHA-256 so a reader
//! can tell whether the copy they kept is the copy they were given.
//!
//! # One request, not a job
//!
//! The per-work exporter is a queued job because a rendered novel is expensive (§13). This archive is
//! a handful of indexed reads over one account's rows; making it a job would mean a reader on a dying
//! instance waiting for a worker that may never run. It is served synchronously and streamed as an
//! attachment.
//!
//! # No work bodies
//!
//! The words travel through the work exporter, where the licence, the CTA and the sanitising already
//! live. Bundling a second copy here would be a second, unaudited path to the same bytes — and the
//! archive's job is to tell a reader what they *had*, not to be a backup of other people's fiction.
//!
//! # It is the account's own
//!
//! There is no id parameter. The only archive a session may read is the one keyed by its own account,
//! which is the same shape §13's exports use and the reason this route cannot be pointed at a stranger.

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::auth::RequireSession;
use crate::http::ApiResult;
use crate::state::AppState;

/// One door, signed in.
pub fn router() -> Router<AppState> {
    Router::new().route("/me/archive", get(download))
}

async fn download(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Response> {
    let account = user.account_id.to_string();
    let sections = darkhold_db::archive::build(state.db(), &account).await?;

    let manifest: Vec<Value> = sections
        .iter()
        .map(|section| {
            json!({
                "name": section.name,
                "count": section.count,
                "sha256": section.sha256,
            })
        })
        .collect();

    let mut body = serde_json::Map::new();
    body.insert("schema".to_owned(), json!(darkhold_db::archive::SCHEMA));
    body.insert(
        "generated_at".to_owned(),
        json!(now_rfc3339()),
    );
    for section in &sections {
        body.insert(section.name.to_owned(), json!(section.rows));
    }
    body.insert("manifest".to_owned(), json!(manifest));

    // The filename names the archive and the day, and never the account: a saved file that carries an
    // account id is a file that identifies its owner in every folder it is ever dropped into.
    let filename = format!("darkhold-archive-{}.json", stamp());
    let disposition = format!("attachment; filename=\"{filename}\"");

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/json; charset=utf-8"),
            (header::CONTENT_DISPOSITION, disposition.as_str()),
            // An archive is a snapshot of a moving account; a cached copy is a stale copy of
            // something a reader asked for *now*.
            (header::CACHE_CONTROL, "no-store"),
        ],
        Json(Value::Object(body)),
    )
        .into_response())
}

/// `YYYY-MM-DD` for the filename, from the same clock the rest of the app uses.
fn stamp() -> String {
    now_rfc3339()
        .split('T')
        .next()
        .unwrap_or("archive")
        .to_owned()
}

/// The instance's clock, in the format every other route writes.
fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}
