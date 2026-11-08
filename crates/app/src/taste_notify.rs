//! The taste-weighted notification queue (§M18 phase 3, migration 0055).
//!
//! `queue_taste_notification`, `drain_taste_notification_queue` and `pending_taste_notification_count` had no caller, so
//! `taste_notification_queue` was a table nothing could fill and nothing could empty. The migration states the intent in
//! one sentence — *"When a work is published, aligned users are queued for notification. The worker drains this table."* —
//! and neither half existed.
//!
//! ## What "aligned" means here, and what it does not
//!
//! The instance's own notion of alignment is the one `taste_vectors` already stores: a reader's distance to the operator's
//! taste centroid. A reader within [`ALIGNED_DISTANCE`] of it is queued for a newly published work with a score of
//! `1 - distance`, which is bounded to `[0, 1]` and means "how close", not "how much we want to sell them something".
//!
//! This is deliberately **not** per-work relevance: darkhold has no accumulated reader-by-work match to rank on (the
//! recommender's evidence is the operator's profile, and §49.9 refuses to invent the rest). A per-work score would be a
//! number nobody could explain, and a notification that arrives for a reason the reader cannot name is worse than none.
//!
//! ## The three refusals, beside the code
//!
//! - **The author is never queued for their own work.** They know it exists.
//! - **A reader already holding [`PENDING_CAP`] unsent notifications is skipped**, which is what
//!   `pending_taste_notification_count` exists for: a queue the worker has not drained must not grow into a backlog that
//!   arrives all at once.
//! - **A disabled module queues nothing.** The producer runs inside the publish path, where a module mount cannot gate it,
//!   so it reads the switch itself.
//!
//! The reader can still refuse all of this in their own settings: the drain goes through `notifications::notify`, which
//! resolves the account's channel and drops the event when the account has turned that kind off.

use darkhold_db::{engagement, notifications, taste_vectors};

use crate::state::AppState;

/// How close to the operator's taste centroid a reader must be to be queued for a new work.
///
/// Measured as distance in the taste vector's own space, so `0.0` is the operator's taste exactly. `0.35` is a wide-but-
/// not-everybody band on purpose: a narrower one would queue almost nobody on a small instance, and a notification nobody
/// receives is indistinguishable from a module that is off.
pub const ALIGNED_DISTANCE: f64 = 0.35;

/// How many unsent taste notifications one reader may hold before the producer skips them.
///
/// Three, because the queue is drained by an operator (there is no scheduler): a reader who is away for a week should find
/// a handful of suggestions, not forty.
pub const PENDING_CAP: i64 = 3;

/// How many candidate readers the producer considers. Bounded so a publish stays a publish.
const CANDIDATES: i64 = 500;

/// What a produce run did, for the publish path to log rather than guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Queued {
    /// Readers queued by this run. Re-queueing an existing `(work, account)` pair is not counted: the store updates the
    /// score and leaves the row, so a second publish of the same work adds no notification.
    pub queued: usize,
    /// Readers inside the band who were skipped because their own queue is full.
    pub skipped_at_cap: usize,
}

/// Queue the readers aligned with the instance's taste for a newly published work.
///
/// Best-effort like the lifecycle events beside it in the publish path: a failure to queue must never fail a publication
/// the author was already told succeeded.
pub async fn queue_aligned_readers(
    state: &AppState,
    work_id: &str,
    author_account: &str,
) -> Result<Queued, anyhow::Error> {
    if !state.modules().enabled("taste_notifications") {
        return Ok(Queued::default());
    }

    let aligned = taste_vectors::list_users_by_centroid_distance(state.db(), CANDIDATES).await?;
    let mut outcome = Queued::default();
    for (account_id, distance) in aligned {
        if account_id == author_account || distance > ALIGNED_DISTANCE {
            continue;
        }
        // The cap is read per candidate rather than in bulk because the queue is keyed by (work, account) and the count
        // is the account's own: one query per inside-the-band reader is a handful of reads on a publish, and the
        // alternative — a GROUP BY over the whole table — is a second definition of "pending" to keep in step.
        let pending = engagement::pending_taste_notification_count(state.db(), &account_id).await?;
        if pending >= PENDING_CAP {
            outcome.skipped_at_cap += 1;
            continue;
        }
        engagement::queue_taste_notification(state.db(), work_id, &account_id, 1.0 - distance).await?;
        outcome.queued += 1;
    }
    Ok(outcome)
}

/// What a drain run did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Drained {
    pub drained: usize,
    pub delivered: usize,
    pub failed: usize,
}

/// Deliver the queued notifications: drain the table, then write one inbox row per reader.
///
/// A **button, not a timer** — there is no scheduler in this binary (§37 puts recurring work on the operator). The drain
/// marks rows sent before delivery, which is the store's design and the right direction: a notification delivered twice is
/// a worse failure than one delivered never, and the row still carries `sent_at` as evidence that it was taken.
pub async fn drain_and_deliver(state: &AppState, limit: i64) -> Result<Drained, anyhow::Error> {
    let rows = engagement::drain_taste_notification_queue(state.db(), limit).await?;
    let mut outcome = Drained {
        drained: rows.len(),
        ..Drained::default()
    };
    for (work_id, account_id, score) in rows {
        // The title names the work, because a notification that says "something you might like" and opens nothing is a
        // notification the reader learns to ignore. A work that vanished between queueing and draining has no title to
        // name — the row is still marked sent (the store did that) and counted as a failure to deliver.
        let title = darkhold_db::content::find_work(state.db(), work_id.parse()?)
            .await
            .ok()
            .flatten()
            .map(|work| work.title);
        let Some(title) = title else {
            tracing::warn!(work = %work_id, account = %account_id, "a queued taste notification names a work that is gone");
            outcome.failed += 1;
            continue;
        };
        let body = format!(
            "\"{title}\" is close to the taste of this instance. You are within {:.0}% of the operator's profile.",
            (1.0 - score) * 100.0
        );
        match notifications::notify(
            state.db(),
            &account_id,
            "taste_suggestion",
            &format!("New: {title}"),
            &body,
            Some(&work_id),
        )
        .await
        {
            Ok(_) => outcome.delivered += 1,
            Err(error) => {
                tracing::warn!(%account_id, %error, "a taste notification could not be delivered");
                outcome.failed += 1;
            }
        }
    }
    // **`delivered` is not a claim that the reader received anything.** `notifications::notify` returns a row id even when
    // the account turned this kind off — the channel lookup decides and drops — so this counts rows written, and the
    // reader's own settings decide whether a row means anything. A separate "suppressed" counter would need a second
    // reading of the same settings, in a second place, that could disagree with the notifier.
    Ok(outcome)
}
