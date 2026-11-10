//! M28 §33.2 — burst detection over the ratings stream.
//!
//! # What was missing, and what this is
//!
//! `rating_anomaly_events` had a reader (`/admin/anomalies` counted it), a clearer (an operator could clear a row) and a
//! writer nobody called (`insert_rating_anomaly_event`). A queue with no producer is a page that says "0 anomalies"
//! forever, which reads exactly like a healthy instance and cannot be told apart from a detector that is not running.
//!
//! This is the producer, deliberately narrow. One rule: **a work whose recent public ratings came from freshly created
//! accounts**. That is the brigade §33.2 names first, and it is the one the schema can answer without inventing a
//! reviewer-profile model. Per-account profile outliers and cohort correlation are the next rules, not this one, and the
//! report says so rather than implying the whole of §33.2 is covered.
//!
//! # Why it is a button and not a timer
//!
//! There is no scheduler in this binary for it to ride: the only recurring loop in the app crate is the discovery pass,
//! and a second `tokio::spawn` per process is a change to how the server starts rather than a detector. So detection runs
//! when an operator asks (`POST /admin/anomalies/detect`) and stores what it finds. The scheduled form is a follow-up
//! with a name in `roadmap-remaining.md`, not a claim this file makes.
//!
//! # The two refusals it keeps
//!
//! * **Detection never marks a work contested.** §33.2 gives the contested mark to a *quorum* of curators, and a detector
//!   that could publish nothing on its own would be a machine moderator. It records an event; a person decides.
//! * **Detection never duplicates an event.** A second run inside the same brigade finds the same works and records
//!   nothing, because `has_uncleared_anomaly` answers first. The alternative is a severity that measures how often
//!   somebody pressed the button.

use chrono::{Duration, Utc};
use darkhold_db::rating_integrity::{self, RatingAnomalyEvent};
use darkhold_domain::ids::WorkId;
use serde_json::json;

use crate::state::AppState;

/// How far back "recent" reaches when looking for a burst.
pub const WINDOW_HOURS: i64 = 6;
/// How young an account must be to count as fresh.
pub const FRESH_ACCOUNT_DAYS: i64 = 7;
/// How many fresh-account ratings inside the window stop being interest and start being a brigade.
pub const MIN_FRESH_RATINGS: i64 = 5;

/// What one detection run did, in the terms an operator can act on.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BurstReport {
    /// When the run looked from.
    pub window_start: String,
    /// Accounts created after this instant counted as fresh.
    pub fresh_accounts_since: String,
    /// The threshold the run used.
    pub min_fresh_ratings: i64,
    /// Works that crossed it in this run.
    pub works_examined: usize,
    /// Events written: works that crossed it and had no uncleared burst event already.
    pub recorded: Vec<RecordedBurst>,
    /// Works that crossed it and already had one.
    pub already_flagged: usize,
}

/// One recorded burst.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RecordedBurst {
    /// The work.
    pub work_id: String,
    /// How many fresh-account ratings it got inside the window.
    pub fresh_ratings: i64,
    /// The event's id.
    pub event_id: String,
}

/// Run burst detection once and record what it finds.
///
/// # Errors
/// Transient for a database failure. A work whose id does not parse as a `WorkId` is skipped rather than failing the run:
/// one malformed row must not cost the other works their turn, the same rule the discovery pass keeps.
pub async fn detect_bursts(state: &AppState) -> anyhow::Result<BurstReport> {
    let now = Utc::now();
    let window_start =
        (now - Duration::hours(WINDOW_HOURS)).to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let account_cutoff = (now - Duration::days(FRESH_ACCOUNT_DAYS))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    let bursts = rating_integrity::works_with_fresh_account_rating_bursts(
        state.db(),
        &window_start,
        &account_cutoff,
        MIN_FRESH_RATINGS,
    )
    .await?;

    let mut recorded = Vec::new();
    let mut already_flagged = 0;
    for (work_id, fresh_ratings) in &bursts {
        if rating_integrity::has_uncleared_anomaly(state.db(), work_id, KIND).await? {
            already_flagged += 1;
            continue;
        }
        let work = match work_id.parse::<WorkId>() {
            Ok(work) => work,
            Err(_) => {
                tracing::warn!(work_id, "a burst was reported for an id that is not a work");
                continue;
            }
        };
        let event_id = uuid::Uuid::new_v4().to_string();
        let event = RatingAnomalyEvent {
            id: event_id.clone(),
            work_id: work.to_string(),
            cohort_id: None,
            kind: KIND.to_string(),
            // The severity is the observation, not a constant: eleven fresh accounts in six hours is a stronger signal
            // than five, and a fixed number would make the two identical to whoever reads the queue.
            severity: *fresh_ratings,
            detail: json!({
                "rule": "fresh_account_burst",  // the detail names the rule; `kind` is the schema's vocabulary
                "fresh_ratings": fresh_ratings,
                "window_hours": WINDOW_HOURS,
                "fresh_account_days": FRESH_ACCOUNT_DAYS,
                "min_fresh_ratings": MIN_FRESH_RATINGS,
            })
            .to_string(),
            detected_at: now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            cleared_at: None,
            cleared_by: None,
        };
        rating_integrity::insert_rating_anomaly_event(state.db(), &event).await?;
        recorded.push(RecordedBurst {
            work_id: work.to_string(),
            fresh_ratings: *fresh_ratings,
            event_id,
        });
    }

    Ok(BurstReport {
        window_start,
        fresh_accounts_since: account_cutoff,
        min_fresh_ratings: MIN_FRESH_RATINGS,
        works_examined: bursts.len(),
        recorded,
        already_flagged,
    })
}

/// The kind string this detector writes.
///
/// `burst`, not a name of my own: migration 0019 constrains `kind` to `('burst', 'cohort_outlier', 'profile_outlier')`,
/// and the schema is the authority on the vocabulary. My first version wrote `fresh_account_burst` and every insert was
/// refused by a CHECK — which is the right outcome, because a queue whose kinds are free text cannot be filtered by the
/// rule that produced them.
pub const KIND: &str = "burst";
