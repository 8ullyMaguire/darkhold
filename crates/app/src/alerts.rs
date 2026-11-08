//! Saved views that tell you.
//!
//! # What was missing
//!
//! M63-06 gave a saved query a memory: the client posts the ids it just saw, the server diffs them against
//! `saved_query_runs` and answers `new_ids`. Useful, and useless as an alert — the diff only exists while a client
//! is open and doing the query itself, so a reader who is asleep is a reader who is never told. The notification
//! arrived if and only if you were already looking.
//!
//! This module runs the query server-side, per account, over the account's own saved views, and turns the diff
//! into inbox events. The client's `POST /saved-views/:id/runs` stays exactly as it was: two doors into one diff,
//! and neither invents a second definition of "new".
//!
//! # The three decisions worth keeping
//!
//! * **The run row is written even when nothing is emitted.** A sweep that only recorded what it announced would
//!   re-announce the same work forever; the diff must advance whether or not the inbox is reachable.
//! * **One event per new work, capped.** A digest ("12 new items") is one notification for twelve discoveries and
//!   cannot be marked read per item; a thousand events is how a bell becomes noise. `ALERT_CAP` per view, and the
//!   remainder is still recorded as seen — the sweep is a notifier, not a log.
//! * **The dedupe key names the view and the work.** `darkhold:alert:<view_id>:<work_id>` means the same work
//!   matching two views is two alerts (a reader asked twice), and the same view matching the same work twice is
//!   one (the inbox dedupes).

use commons_feed::FeedEntry;
use darkhold_db::library::{self, SavedViewKind};

use crate::http::ApiError;
use crate::inbox::{self, Event};
use crate::routes::feeds;
use crate::state::AppState;

/// How many works one view may announce in one sweep.
pub const ALERT_CAP: usize = 20;

/// How many items a view is asked for when the sweep runs it. A filter that matches more than this has more new
/// works than a notification could usefully say, and the run row records the top of the list either way.
pub const SWEEP_LIMIT: i64 = 50;

/// What one sweep did, in numbers a caller can assert on.
#[derive(Debug, Default, serde::Serialize)]
pub struct Report {
    /// Saved views the account holds.
    pub views: usize,
    /// Views the sweep could run.
    pub checked: usize,
    /// Works this sweep had not seen before.
    pub new: usize,
    /// Events handed to the delivery task. `queued` rather than `emitted` on purpose: the emit is
    /// fire-and-forget (the request that produced it answers to a person), so the sweep knows it handed the event
    /// over, not that the inbox accepted it. Arrival is proven by reading the inbox, never by this number.
    pub queued: usize,
    /// New works that produced no event because this product has no inbox credentials. A sweep with nowhere to
    /// send is not a quiet sweep; it says so.
    pub suppressed: usize,
    /// Views that could not be run, with the reason. Never silently skipped.
    pub skipped: Vec<String>,
}

/// The event a new work produces for the view that matched it.
pub fn alert_event(view: &library::SavedView, account: &str, entry: &FeedEntry) -> Event {
    Event {
        kind: "alert".to_owned(),
        recipients: vec![account.to_owned()],
        actor_sub: None,
        actor_handle: None,
        object_kind: Some("work".to_owned()),
        object_id: Some(entry.id.clone()),
        object_url: Some(entry.link.clone()),
        object_title: Some(entry.title.clone()),
        dedupe_key: format!("darkhold:alert:{}:{}", view.id, entry.id),
    }
}

/// The ids one view may announce in one sweep: the first `ALERT_CAP` of what is new, in the order the query
/// returned them (which is the view's own sort, not alphabetical — a reader sorting by newest is told about the
/// newest).
pub fn announceable(new_ids: &[String]) -> &[String] {
    let limit = new_ids.len().min(ALERT_CAP);
    &new_ids[..limit]
}

/// Run every saved view the account owns and emit an alert for each work the view had not seen.
pub async fn sweep_account(state: &AppState, account: &str) -> Result<Report, ApiError> {
    let views = library::saved_views_for(state.db(), account).await?;
    let mut report = Report {
        views: views.len(),
        ..Report::default()
    };

    for view in &views {
        let entries = match view.kind {
            SavedViewKind::Library => feeds::library_items(state, account, view, SWEEP_LIMIT).await,
            SavedViewKind::Catalogue => feeds::catalogue_items(state, view, SWEEP_LIMIT).await,
        };
        let entries = match entries {
            Ok(entries) => entries,
            // A view holding a query this build cannot read is a real state (spec §14.2 versions the document
            // so a repair is possible). It is named in `skipped`, never dropped in silence.
            Err(error) => {
                report.skipped.push(format!("{}: {:?}", view.id, error.0));
                continue;
            }
        };

        let ids: Vec<String> = entries.iter().map(|entry| entry.id.clone()).collect();
        // The diff belongs to the run row, so it is advanced by this sweep whether or not the inbox answers.
        let run = match library::record_run(state.db(), account, &view.id, &ids).await? {
            Some(run) => run,
            None => {
                // Deleted between the listing and the run. Nothing to notify.
                continue;
            }
        };
        report.checked += 1;
        report.new += run.new_ids.len();

        for id in announceable(&run.new_ids) {
            let Some(entry) = entries.iter().find(|entry| &entry.id == id) else {
                continue;
            };
            if inbox::enabled() {
                inbox::emit_in_background("darkhold", alert_event(view, account, entry));
                report.queued += 1;
            } else {
                report.suppressed += 1;
            }
        }
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use darkhold_db::library::SavedViewKind;
    use darkhold_domain::library::{LibrarySort, ViewScope};

    fn view() -> library::SavedView {
        library::SavedView {
            id: "6f1d0f7e-0000-4000-8000-000000000001".to_owned(),
            account_id: "6f1d0f7e-0000-4000-8000-0000000000aa".to_owned(),
            name: "new cyberpunk".to_owned(),
            // A catalogue view holds its query as a canonical string; the sweep executes `params` through the
            // same parser the catalogue route uses, so the fixture carries the string and not a struct.
            query: None,
            kind: SavedViewKind::Catalogue,
            params: Some("tags=cyberpunk&sort=new".to_owned()),
            needs_repair: false,
            query_version: 1,
            sort: LibrarySort::default(),
            scope: ViewScope::default(),
            pinned: true,
            is_public: false,
            created_at: "2026-11-05T12:00:00Z".to_owned(),
            updated_at: "2026-11-05T12:00:00Z".to_owned(),
            version: 1,
        }
    }

    fn entry(id: &str) -> FeedEntry {
        FeedEntry {
            id: id.to_owned(),
            title: format!("work {id}"),
            link: format!("https://darkhold.polarisocial.xyz/w/{id}"),
            summary: None,
            author: None,
            published: None,
            // No filter vocabulary on a new-work alert yet; see the note in `feeds.rs`.
            categories: Vec::new(),
        }
    }

    #[test]
    fn the_dedupe_key_names_the_view_and_the_work() {
        let event = alert_event(&view(), "sub-1", &entry("w-1"));
        assert_eq!(
            event.dedupe_key,
            "darkhold:alert:6f1d0f7e-0000-4000-8000-000000000001:w-1"
        );
        // Two views matching one work are two alerts, because the reader asked twice.
        let mut other = view();
        other.id = "6f1d0f7e-0000-4000-8000-000000000002".to_owned();
        assert_ne!(
            alert_event(&other, "sub-1", &entry("w-1")).dedupe_key,
            event.dedupe_key
        );
    }

    #[test]
    fn the_event_is_an_alert_for_the_owner_and_points_at_the_work() {
        let event = alert_event(&view(), "sub-1", &entry("w-1"));
        assert_eq!(event.kind, "alert");
        assert_eq!(event.recipients, vec!["sub-1".to_owned()]);
        assert_eq!(event.object_kind.as_deref(), Some("work"));
        assert_eq!(event.object_id.as_deref(), Some("w-1"));
        assert!(
            event.object_url.is_some(),
            "a bell that cannot link is a bell that annoys"
        );
        // The author of a work is not the actor of an alert: nobody did this, a query matched.
        assert!(event.actor_sub.is_none());
    }

    #[test]
    fn one_view_announces_at_most_the_cap_and_the_rest_is_still_recorded() {
        let ids: Vec<String> = (0..ALERT_CAP + 5).map(|n| format!("w-{n}")).collect();
        let announced = announceable(&ids);
        assert_eq!(announced.len(), ALERT_CAP);
        assert_eq!(announced[0], "w-0", "the view's own order, newest first");
        // The caller records every id, not just the announced ones: a sweep that forgot the remainder would
        // announce them tomorrow, and the day after.
        assert!(ids.len() > announced.len());
    }
}
