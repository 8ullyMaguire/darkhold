//! M45-70 — explaining a reader's taste, without inventing a number.
//!
//! The ranking already uses these signals; nothing published them. This module turns per-work evidence
//! into per-tag weights **with the signals that produced each**, because a weight alone is a verdict and
//! a reader cannot act on a verdict.
//!
//! Two rules matter and are decided here:
//!
//! * **A signal counts once per work.** A reader who bookmarked a work and also finished it has told us
//!   two things about the same work; a work with twelve tags must not outvote twelve works with one tag
//!   each. `works` is reported per signal so the reader can see the difference between "one work, loudly"
//!   and "six works, quietly".
//! * **A disabled signal is reported, not hidden.** `suppressed` is what the weight *would* have been, in
//!   the opposite direction of the change. Turning a signal off must be visible in the explanation,
//!   otherwise the reader cannot tell whether it did anything.
//!
//! Negative signals are part of the model: a one-star rating is evidence against a tag, and a dropped
//! work is weak evidence against it. A model that only counts enthusiasm explains a reader who dislikes
//! everything as having no taste at all.

use std::collections::BTreeMap;

/// The evidence a reader's own acts provide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Signal {
    /// They finished the work.
    Finished,
    /// They bookmarked it.
    Bookmark,
    /// They rated it four or five stars.
    RatingUp,
    /// They rated it one or two stars.
    RatingDown,
    /// They dropped it.
    Dropped,
}

impl Signal {
    /// Every signal, in the order the explanation lists them.
    pub const ALL: [Signal; 5] = [
        Signal::Finished,
        Signal::Bookmark,
        Signal::RatingUp,
        Signal::RatingDown,
        Signal::Dropped,
    ];

    /// The wire name.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Signal::Finished => "finished",
            Signal::Bookmark => "bookmark",
            Signal::RatingUp => "rating_up",
            Signal::RatingDown => "rating_down",
            Signal::Dropped => "dropped",
        }
    }

    /// What the signal is, in the reader's words.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Signal::Finished => "You finished it",
            Signal::Bookmark => "You bookmarked it",
            Signal::RatingUp => "You rated it 4 or 5 stars",
            Signal::RatingDown => "You rated it 1 or 2 stars",
            Signal::Dropped => "You dropped it",
        }
    }

    /// How much one work carrying a tag, seen through this signal, moves the tag.
    ///
    /// Finishing is the strongest positive act available: it costs time. A bookmark costs a click and a
    /// rating costs a judgement, so both sit below it. The negatives are symmetric in size to the
    /// positives they answer — a one-star rating cancels a four-star one, which is the least surprising
    /// arithmetic a reader can be shown.
    #[must_use]
    pub fn weight(self) -> f64 {
        match self {
            Signal::Finished => 3.0,
            Signal::Bookmark => 2.0,
            Signal::RatingUp => 2.0,
            Signal::RatingDown => -2.0,
            Signal::Dropped => -1.0,
        }
    }

    /// From a stored preference or a query parameter.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|signal| signal.key() == text)
    }
}

/// One act: this work, this signal.
#[derive(Debug, Clone)]
pub struct Event {
    /// The work that was finished, bookmarked or rated.
    pub work: String,
    /// A tag the work carries.
    pub tag: String,
    /// What the reader did.
    pub signal: Signal,
}

/// One signal's share of a tag's weight.
#[derive(Debug, Clone, serde::Serialize)]
pub struct By {
    pub signal: &'static str,
    pub label: &'static str,
    /// Distinct works of this signal carrying the tag.
    pub works: usize,
    /// `works * weight`, which is this signal's contribution.
    pub amount: f64,
    /// Whether the reader has switched this signal off.
    pub disabled: bool,
}

/// A tag, and why it is in the explanation.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ExplainedTag {
    pub tag: String,
    /// What counts right now.
    pub weight: f64,
    /// What the switched-off signals would have contributed, had they counted.
    pub suppressed: f64,
    /// Every signal that touched this tag, heaviest first.
    pub by: Vec<By>,
}

/// Explain `events`, with `disabled` signals reported but not counted.
///
/// The output is ordered by `weight` descending, then `suppressed` descending, then name — so a tag kept
/// alive entirely by a signal the reader switched off still appears, at the bottom of the ranking it
/// deserves, rather than vanishing and leaving them to wonder.
#[must_use]
pub fn explain(events: &[Event], disabled: &[Signal]) -> Vec<ExplainedTag> {
    // (tag, signal) -> distinct works
    let mut works: BTreeMap<(&str, Signal), std::collections::BTreeSet<&str>> = BTreeMap::new();
    for event in events {
        works
            .entry((event.tag.as_str(), event.signal))
            .or_default()
            .insert(event.work.as_str());
    }

    let mut tags: BTreeMap<&str, (f64, f64, Vec<By>)> = BTreeMap::new();
    for ((tag, signal), seen) in works {
        let count = seen.len();
        let amount = count as f64 * signal.weight();
        let off = disabled.contains(&signal);
        let entry = tags.entry(tag).or_insert((0.0, 0.0, Vec::new()));
        if off {
            entry.1 += amount;
        } else {
            entry.0 += amount;
        }
        entry.2.push(By {
            signal: signal.key(),
            label: signal.label(),
            works: count,
            amount: round4(amount),
            disabled: off,
        });
    }

    let mut out: Vec<ExplainedTag> = tags
        .into_iter()
        .filter(|(_, (weight, suppressed, _))| *weight != 0.0 || *suppressed != 0.0)
        .map(|(tag, (weight, suppressed, mut by))| {
            // Heaviest contribution first; ties by signal order so the list is stable between requests.
            by.sort_by(|a, b| {
                b.amount
                    .abs()
                    .total_cmp(&a.amount.abs())
                    .then_with(|| a.signal.cmp(b.signal))
            });
            ExplainedTag {
                tag: tag.to_owned(),
                weight: round4(weight),
                suppressed: round4(suppressed),
                by,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.weight
            .total_cmp(&a.weight)
            .then_with(|| b.suppressed.total_cmp(&a.suppressed))
            .then_with(|| a.tag.cmp(&b.tag))
    });
    out
}

fn round4(value: f64) -> f64 {
    (value * 10_000.0).round() / 10_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(work: &str, tag: &str, signal: Signal) -> Event {
        Event {
            work: work.to_owned(),
            tag: tag.to_owned(),
            signal,
        }
    }

    #[test]
    fn a_work_with_many_tags_does_not_outvote_many_works_with_one() {
        // One finished work carrying three tags, against three finished works each carrying `quiet`.
        let events = vec![
            ev("w1", "loud-a", Signal::Finished),
            ev("w1", "loud-b", Signal::Finished),
            ev("w1", "loud-c", Signal::Finished),
            ev("w2", "quiet", Signal::Finished),
            ev("w3", "quiet", Signal::Finished),
            ev("w4", "quiet", Signal::Finished),
        ];
        let explained = explain(&events, &[]);
        let quiet = explained.iter().find(|t| t.tag == "quiet").expect("quiet");
        assert_eq!(
            quiet.weight, 9.0,
            "three works, three times the weight of one"
        );
        assert_eq!(quiet.by[0].works, 3);
        let loud = explained
            .iter()
            .find(|t| t.tag == "loud-a")
            .expect("loud-a");
        assert_eq!(
            loud.weight, 3.0,
            "one work is one work, however many tags it carries"
        );
    }

    #[test]
    fn one_work_counted_twice_by_the_same_signal_is_one_work() {
        let events = vec![
            ev("w1", "dark", Signal::Bookmark),
            ev("w1", "dark", Signal::Bookmark),
        ];
        let explained = explain(&events, &[]);
        assert_eq!(explained[0].weight, 2.0);
        assert_eq!(explained[0].by[0].works, 1);
    }

    #[test]
    fn a_disabled_signal_is_reported_and_not_counted() {
        let events = vec![
            ev("w1", "dark", Signal::Finished),
            ev("w2", "dark", Signal::Bookmark),
        ];
        let explained = explain(&events, &[Signal::Finished]);
        let dark = &explained[0];
        assert_eq!(dark.weight, 2.0, "only the bookmark counts");
        assert_eq!(
            dark.suppressed, 3.0,
            "and the reader can see what they turned off"
        );
        let finished = dark
            .by
            .iter()
            .find(|b| b.signal == "finished")
            .expect("listed");
        assert!(finished.disabled);
    }

    #[test]
    fn a_disliked_tag_is_negative_not_absent() {
        let events = vec![
            ev("w1", "fluff", Signal::RatingDown),
            ev("w2", "fluff", Signal::RatingDown),
            ev("w3", "dark", Signal::RatingUp),
        ];
        let explained = explain(&events, &[]);
        assert_eq!(explained[0].tag, "dark", "positive first");
        let fluff = explained
            .iter()
            .find(|t| t.tag == "fluff")
            .expect("fluff is present");
        assert_eq!(
            fluff.weight, -4.0,
            "a model that only counts enthusiasm cannot explain dislike"
        );
    }

    #[test]
    fn a_tag_kept_alive_only_by_a_disabled_signal_still_appears() {
        let events = vec![ev("w1", "kink", Signal::Bookmark)];
        let explained = explain(&events, &[Signal::Bookmark]);
        assert_eq!(
            explained.len(),
            1,
            "it must not vanish: the reader needs to see what they turned off"
        );
        assert_eq!(explained[0].weight, 0.0);
        assert_eq!(explained[0].suppressed, 2.0);
    }

    #[test]
    fn signal_keys_round_trip() {
        for signal in Signal::ALL {
            assert_eq!(Signal::parse(signal.key()), Some(signal));
        }
        assert_eq!(Signal::parse("nonsense"), None);
    }
}
