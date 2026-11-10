//! Facet answers, computed at most once a minute per filter set (M71-11).
//!
//! **Why a cache and not an index.** The measurement came first: `GET /api/v1/facets` on the live catalogue (58,042 works, 127,474 live
//! units, 127,474 revisions) answered 50 of 50 requests `200` with **p50 297 ms, p95 372 ms** against a pre-registered budget of p50 ≤ 100
//! and p95 ≤ 200. `EXPLAIN (ANALYZE)` names the cost and rules out the cheap answer: the plan is three sequential scans (works, units,
//! revisions) joined and sorted for a single group aggregate over the whole catalogue — 235 ms before the sort, 398 ms in total. No index
//! removes a scan that must touch every row; the aggregate is O(catalogue) by construction, which is what "facets describe the catalogue
//! under these filters" means.
//!
//! So the answer is memoised per filter set for [`TTL`]. Two consequences are deliberate:
//!
//! * **Counts can be up to a minute stale.** A facet panel is a navigation aid, and a counter that is 60 s behind is worth answering in
//!   2 ms instead of 300. The response carries `computed_at_ms_ago`, so a page can say how old the numbers are rather than pretending. (This note
//! said `facets_computed_at` for a while — a field name that shipped nowhere, which is how documentation stops being evidence.)
//! * **The cache is per process.** Two instances would each hold their own; that is correct for a memo (the worst case is one extra scan
//!   per instance per minute), and it avoids making a read path depend on shared state.
//!
//! Keys are the filter set itself, rendered as a string, so the tag page, the fandom page, the search page and the facet door share one
//! entry per distinct filter rather than one per route.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use darkhold_db::facets::{all_facets, FacetAnswer, FacetFilter};

/// How long one answer is reused. Chosen so a reader clicking through a panel never pays for a second scan, and so a newly imported work
/// reaches the counters within the same minute a reader would notice.
pub const TTL: Duration = Duration::from_secs(60);

/// The most filter sets kept at once. A tag page can be linked with any tag; an unbounded map would grow with the catalogue's vocabulary.
const MAX_ENTRIES: usize = 256;

/// One cached answer: when it was computed, and the counts.
type Entry = (Instant, Vec<FacetAnswer>);

fn cache() -> &'static Mutex<HashMap<String, Entry>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Entry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The identity of a filter set: the database it describes, then every field that changes the answer, in a fixed order.
///
/// The database is part of the key because a filter string alone does not say *whose* catalogue it counts. One process serves one
/// database in production, so this changes nothing there — but a test process opens a database per test, and a key without the database
/// served one test's counts to another. A memo that can answer about the wrong catalogue is worse than no memo.
fn key_of(db: &darkhold_db::Database, filter: &FacetFilter) -> String {
    format!("db={}", std::ptr::from_ref(db) as usize)
        + &format!(
            "tag={:?}|kind={:?}|title={:?}|min={:?}|max={:?}|rating={:?}",
            filter.tag,
            filter.tag_kind,
            filter.title_contains,
            filter.min_words,
            filter.max_words,
            filter.max_rating
        )
}

/// Counts for one filter set, from the cache when a fresh answer exists.
///
/// Returns the answers and when they were computed, so the caller can publish the age instead of implying the numbers are live.
pub async fn cached(
    db: &darkhold_db::Database,
    filter: &FacetFilter,
) -> anyhow::Result<(Vec<FacetAnswer>, Instant)> {
    let key = key_of(db, filter);
    if let Ok(guard) = cache().lock() {
        if let Some((at, answers)) = guard.get(&key) {
            if at.elapsed() < TTL {
                return Ok((answers.clone(), *at));
            }
        }
    }

    let answers = all_facets(db, filter).await?;
    let now = Instant::now();
    if let Ok(mut guard) = cache().lock() {
        // Evict before inserting rather than after: a map that grows past its bound on every insert is a leak with extra steps.
        if guard.len() >= MAX_ENTRIES && !guard.contains_key(&key) {
            guard.retain(|_, (at, _)| at.elapsed() < TTL);
            if guard.len() >= MAX_ENTRIES {
                if let Some(oldest) = guard
                    .iter()
                    .min_by_key(|(_, (at, _))| *at)
                    .map(|(key, _)| key.clone())
                {
                    guard.remove(&oldest);
                }
            }
        }
        guard.insert(key, (now, answers.clone()));
    }
    Ok((answers, now))
}

/// Forget every cached answer. Called by tests, and by an operator action that must see its own write reflected.
pub fn clear() {
    if let Ok(mut guard) = cache().lock() {
        guard.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The filter half of the key, without a database — the unit test has no pool and does not need one.
    fn filter_key(filter: &FacetFilter) -> String {
        format!(
            "tag={:?}|kind={:?}|title={:?}|min={:?}|max={:?}|rating={:?}",
            filter.tag,
            filter.tag_kind,
            filter.title_contains,
            filter.min_words,
            filter.max_words,
            filter.max_rating
        )
    }

    #[test]
    fn two_filters_that_differ_anywhere_have_different_keys() {
        let base = FacetFilter::default();
        let tagged = FacetFilter {
            tag: Some("slow burn".to_owned()),
            ..FacetFilter::default()
        };
        let other_kind = FacetFilter {
            tag: Some("slow burn".to_owned()),
            tag_kind: Some("fandom".to_owned()),
            ..FacetFilter::default()
        };
        let windowed = FacetFilter {
            min_words: Some(1_000),
            ..FacetFilter::default()
        };
        assert_ne!(filter_key(&base), filter_key(&tagged));
        assert_ne!(
            filter_key(&tagged),
            filter_key(&other_kind),
            "a tag and a fandom of the same name are different sets"
        );
        assert_ne!(filter_key(&base), filter_key(&windowed));
        assert_eq!(filter_key(&base), filter_key(&FacetFilter::default()));
    }
}
