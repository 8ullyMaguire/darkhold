//! M69-22 — the morning queue and the daily surprise (Suki §14.6).
//!
//! # Two shapes, two jobs
//!
//! The **queue** is a small, bounded, deliberately *diverse* set. The failure it exists to prevent is a list that is really one
//! fandom wearing five hats: five excellent works, all from the same corner of the archive, read as one recommendation repeated.
//! So the queue takes from the best-represented fandom once, then a different fandom, then back — a round,
//! not a sorted slice — and it stops at a per-fandom cap even when the pool would allow more.
//!
//! The **surprise** is one work, and it must be *the same* work for one reader all day: a surprise that changes on every
//! refresh is not a surprise, it is a slot machine, and the second reload tells the reader the pick was arbitrary. It must also
//! differ between readers, or everyone compares screens and finds the same "surprise". So the pick is a deterministic hash of
//! (day, reader) into the pool, and the pool order is the pool order — no clock, no randomness, no `rand` dependency.
//!
//! # Why these live here and not in a route
//!
//! Both are pure functions of (candidates, size, day, reader). A route that computes them inline cannot be tested without a
//! database and a clock, and the two properties that matter most — stable-within-a-day and different-between-readers — are
//! exactly the ones an accidental dependency on "now" destroys.
//!
//! # What is NOT here
//!
//! The filter is not here. The queue is drawn from a **saved filter**, so the operator retunes it without a deploy and the
//! reader can see the filter behind what they were shown. This module is the selection rule applied to whatever that filter
//! returned; `morning_queue_view` names which saved filter it is.

/// One candidate the queue may offer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// The work (or library item) id.
    pub work_id: String,
    /// The title, for the reader.
    pub title: String,
    /// The work's first fandom, when one is known. Empty means unknown, and unknown items share one bucket rather than each
    /// becoming its own — otherwise a catalogue of untagged works would look perfectly diverse.
    pub fandom: String,
}

impl Item {
    /// The bucket this item is counted against.
    #[must_use]
    pub fn bucket(&self) -> &str {
        if self.fandom.trim().is_empty() {
            "unknown"
        } else {
            self.fandom.trim()
        }
    }
}

/// How many items the queue offers at most.
pub const DEFAULT_QUEUE_SIZE: usize = 5;

/// How many items from one fandom the queue may contain, whatever the size.
///
/// Two is the smallest number that lets a queue say "and here is another from the same place" while still being a queue rather
/// than a list. Anything above two and one fandom can take half a five-work queue.
pub const DEFAULT_PER_FANDOM: usize = 2;

/// Take a diverse, bounded slice of `candidates`, keeping the candidates' own order inside each fandom.
///
/// The order the pool arrives in is the ranking the saved filter asked for, so "first from each fandom, then second from each"
/// is the ranking interrupted, not replaced: the best work in the pool is still first.
#[must_use]
pub fn diversify(candidates: &[Item], size: usize, per_fandom: usize) -> Vec<Item> {
    let size = size.max(1);
    let per_fandom = per_fandom.max(1);

    // Buckets in first-appearance order, so the round is deterministic and follows the pool's ranking rather than an
    // alphabetical accident.
    let mut buckets: Vec<(String, Vec<&Item>)> = Vec::new();
    for item in candidates {
        let bucket = item.bucket().to_owned();
        match buckets.iter_mut().find(|(name, _)| name == &bucket) {
            Some((_, items)) => items.push(item),
            None => buckets.push((bucket, vec![item])),
        }
    }

    let mut taken: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    let mut out: Vec<Item> = Vec::with_capacity(size);
    // Rounds: 0 takes the first of every bucket, 1 the second of every bucket that has one, and so on, stopping at the cap.
    for round in 0..per_fandom {
        for (name, items) in &buckets {
            if out.len() >= size {
                return out;
            }
            let Some(item) = items.get(round) else {
                continue;
            };
            let count = taken.entry(name.as_str()).or_default();
            // Cannot be reached with `round < per_fandom`, but a cap that silently stops capping is worse than a check.
            if *count >= per_fandom {
                continue;
            }
            *count += 1;
            out.push((*item).clone());
        }
    }
    // A pool smaller than `size` is not padded, and a pool whose tail is one fandom is not forced: if the cap left the queue
    // short and there are candidates left, fill from what remains in pool order. The cap applies to a *diverse* choice, not to
    // a short one.
    if out.len() < size {
        for item in candidates {
            if out.len() >= size {
                break;
            }
            if !out.iter().any(|chosen| chosen.work_id == item.work_id) {
                out.push(item.clone());
            }
        }
    }
    out
}

/// A stable hash of (day, reader): FNV-1a over the joined text.
///
/// Chosen over `DefaultHasher` because `DefaultHasher` is explicitly not stable across Rust releases, and a surprise that
/// changes when the binary is upgraded is not stable within the day it was promised for.
fn day_seed(day: &str, reader: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in day.as_bytes().iter().chain(b"/").chain(reader.as_bytes()) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Today's surprise for one reader: the same work all day, a different draw between readers.
///
/// A reader with nothing to draw from gets `None` — the honest answer, and the one a route can render as "nothing today"
/// rather than a random work from an unrelated pool.
#[must_use]
pub fn surprise(candidates: &[Item], day: &str, reader: &str) -> Option<Item> {
    if candidates.is_empty() {
        return None;
    }
    let seed = day_seed(day.trim(), reader.trim());
    let index = (seed % candidates.len() as u64) as usize;
    Some(candidates[index].clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, fandom: &str) -> Item {
        Item {
            work_id: id.to_owned(),
            title: format!("Work {id}"),
            fandom: fandom.to_owned(),
        }
    }

    #[test]
    fn the_best_work_is_first_and_the_rest_interleave() {
        let pool = vec![
            item("a1", "Storm"),
            item("a2", "Storm"),
            item("a3", "Storm"),
            item("b1", "Quiet"),
            item("c1", "Harbour"),
        ];
        let queue = diversify(&pool, 5, 2);
        assert_eq!(queue.len(), 5);
        assert_eq!(queue[0].work_id, "a1", "the pool's first is still first");
        let order: Vec<&str> = queue.iter().map(|row| row.work_id.as_str()).collect();
        assert_eq!(order, vec!["a1", "b1", "c1", "a2", "a3"]);
    }

    #[test]
    fn one_fandom_cannot_take_a_whole_queue() {
        let pool: Vec<Item> = (0..5)
            .map(|index| item(&format!("x{index}"), "Same"))
            .collect();
        let queue = diversify(&pool, 5, 2);
        assert_eq!(
            queue.len(),
            5,
            "a short pool is filled rather than padded to nothing"
        );
        // With only one fandom there is nothing to interleave: the cap cannot be honoured and the queue still has to be
        // useful, so it fills in pool order and the response says nothing about diversity it does not have.
        assert_eq!(queue[0].work_id, "x0");
    }

    #[test]
    fn a_full_pool_respects_the_cap_per_fandom() {
        let pool = vec![
            item("a1", "Storm"),
            item("a2", "Storm"),
            item("a3", "Storm"),
            item("a4", "Storm"),
            item("b1", "Quiet"),
            item("b2", "Quiet"),
            item("c1", "Harbour"),
            item("c2", "Harbour"),
        ];
        let queue = diversify(&pool, 5, 2);
        let storms = queue.iter().filter(|row| row.fandom == "Storm").count();
        assert_eq!(storms, 2, "two rounds of every bucket, no more: {queue:?}");
        assert_eq!(queue.len(), 5);
        let order: Vec<&str> = queue.iter().map(|row| row.work_id.as_str()).collect();
        assert_eq!(order, vec!["a1", "b1", "c1", "a2", "b2"]);
    }

    #[test]
    fn unknown_fandoms_share_one_bucket_rather_than_looking_diverse() {
        let pool = vec![
            item("a1", ""),
            item("a2", "   "),
            item("b1", "Quiet"),
            item("c1", "Harbour"),
        ];
        let queue = diversify(&pool, 4, 2);
        assert_eq!(queue[0].bucket(), "unknown");
        let unknowns = queue.iter().filter(|row| row.bucket() == "unknown").count();
        assert_eq!(unknowns, 2, "untagged works are one bucket: {queue:?}");
    }

    #[test]
    fn an_empty_pool_is_an_empty_queue_not_a_panic() {
        assert!(diversify(&[], 5, 2).is_empty());
        assert!(surprise(&[], "2026-11-07", "reader").is_none());
    }

    #[test]
    fn the_surprise_is_stable_all_day_and_different_between_readers() {
        let pool: Vec<Item> = (0..20)
            .map(|index| item(&format!("w{index}"), "Storm"))
            .collect();
        let first = surprise(&pool, "2026-11-07", "alice").expect("a pick");
        let again = surprise(&pool, "2026-11-07", "alice").expect("a pick");
        assert_eq!(first, again, "the same reader, the same day, the same work");

        let tomorrow = surprise(&pool, "2026-11-08", "alice").expect("a pick");
        assert_ne!(first, tomorrow, "a new day is a new draw");

        // Over twenty readers on one day the draws must not all land on one work: that is the whole point of seeding by
        // reader, and a seed that ignored the reader would pass a single-reader test.
        let picks: std::collections::BTreeSet<String> = (0..20)
            .map(|index| {
                surprise(&pool, "2026-11-07", &format!("reader-{index}"))
                    .expect("a pick")
                    .work_id
            })
            .collect();
        assert!(
            picks.len() >= 8,
            "twenty readers, distinct draws: {picks:?}"
        );
    }

    #[test]
    fn a_size_of_zero_still_offers_one_work() {
        let pool = vec![item("a", "Storm")];
        assert_eq!(diversify(&pool, 0, 2).len(), 1);
    }
}
