//! Recording warmth: the writer §41.2's audience panel never had.
//!
//! `GET /me/audience` aggregates `interaction_warmth`, and `longevity::record_warmth` writes it. The writer had **no caller**, so
//! the deployed instance held **0 rows** and every author's panel answered five zeroes — a number that cannot distinguish "nobody
//! reads me" from "nothing is recorded", on an instance with 58,042 works. The rule itself was never missing: `warmth_delta` names
//! the basis points per action (ReadUnit 100, FinishWork 500, React 200, Comment 400) and `tier_for` maps the score to a tier.
//!
//! Three decisions, each recorded beside the code:
//!
//! * **Best-effort.** A warmth failure is a `warn!` and never an error on the reader's action. A reader's kudos must not 500 because
//!   a derived panel could not be updated; the panel is a report, not the fact.
//! * **A reader cannot warm themselves.** The author acting on their own work is skipped: the panel's `create` tier would otherwise
//!   count the one person guaranteed to be in it, and §41.2's tiers are about *readers*.
//! * **The threshold config is read, not decorative.** `CommunityConfig::warmth_thresholds` was a `serde_json::Value` that nothing
//!   parsed, so the operator knob changed nothing. A malformed value falls back to the documented defaults **and says so** — an
//!   operator who mistyped a threshold should learn it from a log line, not from a panel that quietly used a different number.

use darkhold_domain::longevity::{warmth_delta, WarmthAction, WarmthThresholds};

use crate::state::AppState;

/// The warmth thresholds in force, from config, with the documented defaults as the fallback.
///
/// `CommunityConfig::warmth_thresholds` is stored as JSON so an operator can override one tier without a schema change. Reading it
/// here (rather than trusting it) is the point: a knob with no reader is a knob that does nothing, and this file was written because
/// the *table* had no writer for exactly that reason.
pub fn thresholds(state: &AppState) -> WarmthThresholds {
    let raw = &state.config().community.warmth_thresholds;
    let defaults = WarmthThresholds::default();
    let read = |key: &str, default: i64| -> i64 {
        match raw.get(key).and_then(serde_json::Value::as_i64) {
            Some(value) => value,
            None => {
                tracing::warn!(
                    key,
                    default,
                    "warmth threshold is missing or not an integer; using the documented default"
                );
                default
            }
        }
    };
    WarmthThresholds {
        lurk: read("lurk", defaults.lurk),
        react: read("react", defaults.react),
        comment: read("comment", defaults.comment),
        create: read("create", defaults.create),
    }
}

/// The account that owns a work, resolved once for both directions.
async fn resolve_author(state: &AppState, work_id: &str) -> Option<String> {
    let Ok(work) = work_id.parse::<darkhold_domain::WorkId>() else {
        tracing::debug!(work_id, "not a work id; warmth not recorded");
        return None;
    };
    match darkhold_db::positivity::author_account_for_work(state.db(), work).await {
        Ok(Some(author)) => Some(author.to_string()),
        Ok(None) => {
            tracing::debug!(
                work_id,
                "no author account for this work; warmth not recorded"
            );
            None
        }
        Err(error) => {
            tracing::warn!(%error, work_id, "could not resolve the author for warmth");
            None
        }
    }
}

/// Add (or, with a negative delta, remove) warmth between a reader and a work's author.
///
/// `action` is the reader's act and the delta comes from the domain, so two call sites cannot disagree about what a kudos is worth.
/// Self-interaction is skipped rather than recorded with a zero delta, because a row would still show the author in their own panel.
pub async fn record(state: &AppState, reader_account: &str, work_id: &str, action: WarmthAction) {
    // The author lookup is `positivity`'s, not a second join of my own: it already excludes soft-deleted works, and a second
    // SQL statement for the same question is how two surfaces start disagreeing about who wrote something.
    let author = match resolve_author(state, work_id).await {
        Some(author) => author,
        None => return,
    };
    if author == reader_account {
        tracing::debug!(work_id, "self-interaction is not warmth");
        return;
    }
    if let Err(error) = darkhold_db::longevity::record_warmth(
        state.db(),
        reader_account,
        &author,
        warmth_delta(action),
        &thresholds(state),
    )
    .await
    {
        // Best-effort: the reader's act already happened and already answered.
        tracing::warn!(%error, work_id, "warmth could not be recorded");
    }
}

/// The same, with the delta reversed — used where the act can be undone (the kudos toggle).
///
/// Symmetry matters: a reader who presses the button twice must not end up warmer than one who presses it once, and an additive
/// score with an add-only writer makes the second press free warmth.
pub async fn record_removal(
    state: &AppState,
    reader_account: &str,
    work_id: &str,
    action: WarmthAction,
) {
    let author = match resolve_author(state, work_id).await {
        Some(author) => author,
        None => return,
    };
    if author == reader_account {
        return;
    }
    if let Err(error) = darkhold_db::longevity::record_warmth(
        state.db(),
        reader_account,
        &author,
        -warmth_delta(action),
        &thresholds(state),
    )
    .await
    {
        tracing::warn!(%error, work_id, "warmth could not be removed");
    }
}
