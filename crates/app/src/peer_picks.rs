//! Sister instances' public picks (M45-44, spec §0.4 "sister instances").
//!
//! Two directions, one vocabulary:
//!
//! * **publish** — `GET /federation/picks` answers an ActivityStreams `OrderedCollection` of the works this instance recommends;
//! * **subscribe** — an operator names a peer's collection URL, a sweep fetches it, and the titles land here.
//!
//! # What a "pick" is, and why that definition
//!
//! A pick is a **public catalogue row this instance would show a stranger**, ordered by the ranking the catalogue already uses
//! for its default (`Aggregate`: every source's likes plus this instance's own kudos). That is a decision, not an obvious one, so
//! the alternatives it beat:
//!
//! * *operator-curated picks* need an editorial surface that does not exist yet (§16.11), and inventing one here would make a
//!   federation feature depend on an unbuilt admin UI;
//! * *this instance's own works only* would make the collection nearly empty on an instance that has imported more than it
//!   publishes, and a peer subscribing gets nothing — the failure mode that looks like a working feature;
//! * *the discovery feed for an anonymous reader* is personalised by trend and freshness, which is a number that changes hourly;
//!   a collection a peer fetches should be stable between fetches or every poll reads as a new set of picks.
//!
//! The eligibility is the catalogue's own (`catalogue::search`), so a restricted or deleted work cannot appear here for the same
//! reason it cannot appear on the page (M71-16).
//!
//! # What this does not do
//!
//! It does not verify the *peer's* signature on their collection (the fetch is a plain GET; the collection is a public document),
//! and it does not reconcile a peer pick with a local work — a pick that matches something in this library is still shown as the
//! peer's, by URL. Both are deliberate: attributing a pick to a peer is the point, and guessing an identity match would turn
//! "they recommend this" into "we think this is the same thing", which is the peering question (M69-05) asked badly.

use std::collections::BTreeSet;

use darkhold_db::Database;
use serde_json::{json, Value};

use crate::state::AppState;

/// How many works the instance publishes in its collection.
///
/// Fifty: enough to be a list somebody reads, small enough that a peer polling every few minutes is not moving a large document
/// to learn nothing changed.
pub const PICKS_LIMIT: i64 = 50;

/// How many subscriptions one sweep pass refreshes.
///
/// Bounded because each one is an outbound request inside a worker tick: an instance that subscribes to thirty peers must not
/// discover that a single pass takes longer than the interval between passes.
pub const SWEEP_BATCH: i64 = 5;

/// One work as the collection carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickItem {
    pub id: String,
    pub title: String,
    pub author: Option<String>,
}

/// This instance's public picks, as an ActivityStreams `OrderedCollection`.
///
/// `id`/`first` are the document's own URL, so a peer that follows `first` finds the same list rather than a second one with a
/// different shape. `totalItems` is the count actually carried: a collection that claims more than it lists is a document that
/// lies about its own length, which is the one thing a consumer checks.
pub async fn publish(state: &AppState) -> Result<Value, anyhow::Error> {
    let base = state.config().site.base_url.clone();
    let query = darkhold_db::catalogue::CatalogueQuery {
        sort: Some(darkhold_db::catalogue::CatalogueSort::Aggregate),
        limit: Some(PICKS_LIMIT),
        ..Default::default()
    };
    let rows = darkhold_db::catalogue::search(state.db(), &query).await?;
    let items: Vec<Value> = rows
        .into_iter()
        .map(|row| {
            json!({
                "type": "Document",
                // The catalogue's public URL for the work, which is what a peer can link to and what a reader can open.
                "id": row.source_url,
                "name": row.title,
                "attributedTo": row.author_text,
            })
        })
        .collect();
    Ok(json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "type": "OrderedCollection",
        "id": format!("{base}/federation/picks"),
        "first": format!("{base}/federation/picks"),
        "totalItems": items.len(),
        "orderedItems": items,
    }))
}

/// Read a peer's collection into pick items.
///
/// Accepts both shapes a peer might publish: the ActivityStreams `orderedItems`/`items` array this module emits, and a bare
/// array of objects. Unknown entries are skipped rather than refused — one malformed item must not discard a whole collection,
/// and a peer whose document is entirely malformed yields the empty list, which the caller treats as a successful fetch of
/// nothing rather than an error (an empty collection is a legitimate answer).
pub fn parse_collection(document: &Value) -> Vec<PickItem> {
    let array = document
        .get("orderedItems")
        .or_else(|| document.get("items"))
        .and_then(Value::as_array)
        // A bare array of items is the other shape: `[{"id":…}]` with no wrapper.
        .or_else(|| document.as_array());
    let Some(array) = array else {
        return Vec::new();
    };
    let mut seen = BTreeSet::new();
    array
        .iter()
        .filter_map(|item| {
            // A pick must be an object with an id and a title. An object without a title is skipped rather than shown as a bare
            // link: a title is what a reader reads, and the `?` here is "this entry is not a pick", not a failure.
            let object = item.as_object()?;
            let id = object.get("id").and_then(Value::as_str)?;
            let title = object
                .get("name")
                .or_else(|| object.get("title"))
                .and_then(Value::as_str)?;
            let author = object
                .get("attributedTo")
                .or_else(|| object.get("author"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            // The same work twice in one collection is one pick: a peer listing it under two headings must not spend two of the
            // reader's rows on it.
            if !seen.insert(id.to_owned()) {
                return None;
            }
            Some(PickItem {
                id: id.to_owned(),
                title: title.to_owned(),
                author,
            })
        })
        .collect()
}

/// Fetch one peer's collection and store it.
///
/// Returns the status line recorded on the subscription. A failure is returned as `Err` so the caller can count it, and the
/// subscription's own row says what happened — an operator reading the list should not have to correlate it with a log.
pub async fn refresh_one(state: &AppState, sub_id: &str, url: &str) -> Result<usize, String> {
    match fetch_and_store(state, sub_id, url).await {
        Ok(count) => {
            record(state.db(), sub_id, &format!("{count} picks"), true).await;
            Ok(count)
        }
        Err(reason) => {
            // Every failure path records, not only the one that got as far as an HTTP status: a peer whose host does not resolve
            // is exactly the case an operator needs to see in the list, and it never reaches a response to check.
            record(state.db(), sub_id, &reason, false).await;
            Err(reason)
        }
    }
}

async fn fetch_and_store(state: &AppState, sub_id: &str, url: &str) -> Result<usize, String> {
    let response = state
        .reqwest_client()
        .get(url)
        .header(
            reqwest::header::ACCEPT,
            "application/activity+json, application/json",
        )
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|error| format!("could not reach the peer: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("the peer answered {}", response.status()));
    }
    let document: Value = response
        .json()
        .await
        .map_err(|error| format!("the collection is not JSON: {error}"))?;
    let items = parse_collection(&document);
    darkhold_db::peer_picks::replace_picks(
        state.db(),
        sub_id,
        &items
            .iter()
            .map(|item| (item.id.clone(), item.title.clone(), item.author.clone()))
            .collect::<Vec<_>>(),
    )
    .await
    .map_err(|error| format!("storing the picks failed: {error}"))
}

async fn record(db: &Database, sub_id: &str, status: &str, ok: bool) {
    if let Err(error) = darkhold_db::peer_picks::record_fetch(db, sub_id, status, ok).await {
        tracing::warn!(%error, "recording a peer-picks fetch failed");
    }
}

/// One sweep pass: refresh the subscriptions that have waited longest.
///
/// Never fails the pass: a peer being down is an expected condition of federation, not a reason to stop fetching the others.
pub async fn sweep(state: &AppState) -> usize {
    let due = match darkhold_db::peer_picks::due_subscriptions(state.db(), SWEEP_BATCH).await {
        Ok(due) => due,
        Err(error) => {
            tracing::warn!(%error, "could not list peer-picks subscriptions");
            return 0;
        }
    };
    let mut refreshed = 0;
    for (id, host, url) in due {
        match refresh_one(state, &id, &url).await {
            Ok(count) => {
                refreshed += 1;
                tracing::info!(%host, count, "peer picks refreshed");
            }
            Err(reason) => {
                tracing::warn!(%host, %reason, "peer picks could not be refreshed");
            }
        }
    }
    refreshed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_collection_is_read_in_both_shapes_and_deduplicated() {
        let wrapped = json!({
            "type": "OrderedCollection",
            "orderedItems": [
                { "id": "https://a.example/w/1", "name": "First", "attributedTo": "someone" },
                { "id": "https://a.example/w/1", "name": "First again" },
                { "id": "https://a.example/w/2", "name": "Second" },
                { "name": "no id at all" },
                "https://a.example/w/3"
            ]
        });
        let items = parse_collection(&wrapped);
        assert_eq!(items.len(), 2, "{items:?}");
        assert_eq!(items[0].title, "First", "the first entry wins");
        assert_eq!(items[0].author.as_deref(), Some("someone"));
        assert_eq!(items[1].id, "https://a.example/w/2");

        let bare = json!([{ "id": "https://b.example/w/9", "title": "Bare" }]);
        let items = parse_collection(&bare);
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].title, "Bare",
            "`title` is accepted as well as `name`"
        );
        assert!(items[0].author.is_none());

        // A document with no items is an empty collection, not an error: a peer recommending nothing is a valid answer.
        assert!(parse_collection(&json!({ "type": "OrderedCollection" })).is_empty());
    }
}
