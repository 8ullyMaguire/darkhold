//! M69-07 — the evacuation plan: who takes what, and the receipt that stops a work forking.
//!
//! # The plan is a pure function of two lists
//!
//! Works to place, recipients to place them with. Everything that makes the plan defensible is a property of the algorithm
//! rather than of the call site: closest taste first (the recipients come from `mirroring::eligible_peers`, already ranked and
//! already veto-cleared), balanced by count so one close peer does not inherit an entire instance, and fully deterministic so
//! two operators re-running the same plan get the same answer.
//!
//! # The fork is the failure this module exists to prevent
//!
//! Two peers both accepting the same work means two authoritative copies, and the author's page becomes two pages with one
//! name on them. Nothing here can stop a peer from *claiming* it — the ring cannot see what a peer does after the handover —
//! but the plan can refuse to place one work twice, and [`Receipt::forks`] can name the ones that ended up with two acceptors
//! anyway so an operator sees the fork while it is still a data problem rather than after it is a reader-facing one.
//!
//! # Balance is a rule, not a preference
//!
//! Assigning every work to the closest peer is the naive plan and a bad one: the closest peer is the one that is closest to
//! *everything*, so it takes everything, and an evacuation that concentrates an instance's whole catalogue on one host has
//! recreated the single point of failure it was performed to avoid. So the plan fills the least-loaded eligible peer first
//! among those within [`BALANCE_WINDOW`] of the best score, and only then by score. Beyond that window the score wins: a
//! peer much further away does not get work to keep the counts even.

use crate::mirroring::Eligible;

/// How much worse than the closest peer a recipient may be and still be preferred for balance.
///
/// 0.15 of cosine similarity: inside this band the peers are close enough that spreading the load costs nothing an author
/// would notice. Outside it, taste wins.
pub const BALANCE_WINDOW: f64 = 0.15;

/// A work that needs somewhere to live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkNeed {
    /// The work's id.
    pub work_id: String,
    /// Its title, for the receipt a human reads.
    pub title: String,
}

/// Where a work is going, or why it is not going anywhere.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    /// The work.
    pub work_id: String,
    /// The work's title.
    pub title: String,
    /// The recipient.
    pub recipient: String,
    /// That recipient's taste similarity to this instance.
    pub similarity: f64,
}

/// A work the plan could not place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unplaced {
    /// The work.
    pub work_id: String,
    /// The work's title.
    pub title: String,
    /// Why it has no recipient.
    pub reason: String,
}

/// The plan.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Plan {
    /// Where each work went, in the order the works were given.
    pub placements: Vec<Placement>,
    /// The works with nowhere to go.
    pub unplaced: Vec<Unplaced>,
}

impl Plan {
    /// How many works each recipient took, best-loaded first, ties by name.
    #[must_use]
    pub fn load(&self) -> Vec<(String, usize)> {
        let mut counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
        for placement in &self.placements {
            *counts.entry(placement.recipient.as_str()).or_default() += 1;
        }
        let mut out: Vec<(String, usize)> = counts
            .into_iter()
            .map(|(instance, count)| (instance.to_owned(), count))
            .collect();
        out.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
        out
    }
}

/// Plan an evacuation.
///
/// `recipients` must already be eligible (`mirroring::eligible_peers` filtered them); this function does not second-guess the
/// veto, because two places that decide whether a peer may hold a copy is one too many.
#[must_use]
pub fn plan(works: &[WorkNeed], recipients: &[Eligible]) -> Plan {
    let mut out = Plan::default();
    if recipients.is_empty() {
        out.unplaced = works
            .iter()
            .map(|work| Unplaced {
                work_id: work.work_id.clone(),
                title: work.title.clone(),
                reason: "no peer is eligible to receive this instance's work".to_owned(),
            })
            .collect();
        return out;
    }

    // Descending by similarity, ties by instance id: the same input must produce the same plan on every run.
    let mut ranked: Vec<&Eligible> = recipients.iter().collect();
    ranked.sort_by(|left, right| {
        right
            .similarity
            .partial_cmp(&left.similarity)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.instance_id.cmp(&right.instance_id))
    });
    let best = ranked[0].similarity;

    let mut load: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for work in works {
        // Candidates inside the balance window, least loaded first, then closest.
        let mut candidates: Vec<&&Eligible> = ranked
            .iter()
            .filter(|peer| peer.similarity >= best - BALANCE_WINDOW)
            .collect();
        candidates.sort_by(|left, right| {
            let left_load = load.get(left.instance_id.as_str()).copied().unwrap_or(0);
            let right_load = load.get(right.instance_id.as_str()).copied().unwrap_or(0);
            left_load
                .cmp(&right_load)
                .then_with(|| {
                    right
                        .similarity
                        .partial_cmp(&left.similarity)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .then_with(|| left.instance_id.cmp(&right.instance_id))
        });
        let Some(chosen) = candidates.first() else {
            // Unreachable given `best` is always inside its own window, but a `None` here would be a silent drop.
            out.unplaced.push(Unplaced {
                work_id: work.work_id.clone(),
                title: work.title.clone(),
                reason: "no recipient was inside the balance window".to_owned(),
            });
            continue;
        };
        *load.entry(chosen.instance_id.as_str()).or_default() += 1;
        out.placements.push(Placement {
            work_id: work.work_id.clone(),
            title: work.title.clone(),
            recipient: chosen.instance_id.clone(),
            similarity: chosen.similarity,
        });
    }
    out
}

/// One line of the receipt: who took what, as recorded when the recipient confirmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Acceptance {
    /// The work.
    pub work_id: String,
    /// The work's title.
    pub title: String,
    /// The recipient that confirmed it.
    pub recipient: String,
}

/// What actually happened, as opposed to what was planned.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Receipt {
    /// Who took what.
    pub accepted: Vec<Acceptance>,
    /// Works offered but not accepted.
    pub declined: Vec<Unplaced>,
}

impl Receipt {
    /// Works that more than one recipient accepted, sorted, with the receivers named.
    ///
    /// This is the query that matters after a shutdown: an empty list means one work, one home. A non-empty one is a page
    /// about to exist in two places under one author, and the operator has to choose which receipt stands.
    #[must_use]
    pub fn forks(&self) -> Vec<(String, Vec<String>)> {
        let mut by_work: std::collections::BTreeMap<&str, Vec<&str>> =
            std::collections::BTreeMap::new();
        for acceptance in &self.accepted {
            by_work
                .entry(acceptance.work_id.as_str())
                .or_default()
                .push(acceptance.recipient.as_str());
        }
        by_work
            .into_iter()
            .filter(|(_, recipients)| recipients.len() > 1)
            .map(|(work_id, mut recipients)| {
                recipients.sort_unstable();
                recipients.dedup();
                (
                    work_id.to_owned(),
                    recipients
                        .into_iter()
                        .map(str::to_owned)
                        .collect::<Vec<String>>(),
                )
            })
            .filter(|(_, recipients)| recipients.len() > 1)
            .collect()
    }

    /// How many works each recipient took.
    #[must_use]
    pub fn load(&self) -> Vec<(String, usize)> {
        let mut counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
        for acceptance in &self.accepted {
            *counts.entry(acceptance.recipient.as_str()).or_default() += 1;
        }
        let mut out: Vec<(String, usize)> = counts
            .into_iter()
            .map(|(instance, count)| (instance.to_owned(), count))
            .collect();
        out.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recipients() -> Vec<Eligible> {
        vec![
            Eligible {
                instance_id: "https://close.example".to_owned(),
                similarity: 0.92,
                measured: true,
            },
            Eligible {
                instance_id: "https://near.example".to_owned(),
                similarity: 0.88,
                measured: true,
            },
            Eligible {
                instance_id: "https://far.example".to_owned(),
                similarity: 0.30,
                measured: true,
            },
        ]
    }

    fn works(count: usize) -> Vec<WorkNeed> {
        (0..count)
            .map(|index| WorkNeed {
                work_id: format!("work-{index}"),
                title: format!("Work {index}"),
            })
            .collect()
    }

    #[test]
    fn every_work_gets_exactly_one_recipient() {
        let plan = plan(&works(10), &recipients());
        assert_eq!(plan.placements.len(), 10);
        assert!(plan.unplaced.is_empty());
        let mut ids: Vec<&str> = plan
            .placements
            .iter()
            .map(|row| row.work_id.as_str())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 10, "one placement per work, never two");
    }

    #[test]
    fn the_load_is_spread_inside_the_band_and_never_lands_on_the_distant_peer() {
        let plan = plan(&works(10), &recipients());
        let load = plan.load();
        // The two peers inside 0.15 of the best share the lot; concentrating everything on the closest would recreate the
        // single point of failure the evacuation exists to avoid.
        assert_eq!(load.len(), 2, "{load:?}");
        assert!(load.iter().all(|(_, count)| *count == 5), "{load:?}");
        assert!(
            !plan
                .placements
                .iter()
                .any(|row| row.recipient == "https://far.example"),
            "a peer 0.62 away does not get work to keep the counts even"
        );
    }

    #[test]
    fn a_peer_alone_inside_the_band_takes_everything() {
        let only = vec![Eligible {
            instance_id: "https://solo.example".to_owned(),
            similarity: 0.7,
            measured: true,
        }];
        let plan = plan(&works(3), &only);
        assert_eq!(plan.placements.len(), 3);
        assert!(plan
            .placements
            .iter()
            .all(|row| row.recipient == "https://solo.example"));
    }

    #[test]
    fn no_recipients_is_every_work_unplaced_with_a_reason() {
        let plan = plan(&works(2), &[]);
        assert!(plan.placements.is_empty());
        assert_eq!(plan.unplaced.len(), 2);
        assert!(plan.unplaced[0].reason.contains("no peer is eligible"));
    }

    #[test]
    fn the_plan_is_reproducible_regardless_of_input_order() {
        let mut shuffled = recipients();
        shuffled.reverse();
        let first = plan(&works(9), &recipients());
        let second = plan(&works(9), &shuffled);
        assert_eq!(
            first.placements, second.placements,
            "the same works and peers must give the same plan, or two operators disagree about who took what"
        );
    }

    #[test]
    fn a_receipt_with_two_acceptors_of_one_work_names_the_fork() {
        let receipt = Receipt {
            accepted: vec![
                Acceptance {
                    work_id: "work-1".to_owned(),
                    title: "The Storm".to_owned(),
                    recipient: "https://b.example".to_owned(),
                },
                Acceptance {
                    work_id: "work-1".to_owned(),
                    title: "The Storm".to_owned(),
                    recipient: "https://a.example".to_owned(),
                },
                Acceptance {
                    work_id: "work-2".to_owned(),
                    title: "Another".to_owned(),
                    recipient: "https://a.example".to_owned(),
                },
            ],
            declined: Vec::new(),
        };
        let forks = receipt.forks();
        assert_eq!(forks.len(), 1, "{forks:?}");
        assert_eq!(forks[0].0, "work-1");
        assert_eq!(forks[0].1, vec!["https://a.example", "https://b.example"]);
        let load = receipt.load();
        assert_eq!(load[0], ("https://a.example".to_owned(), 2));
    }

    #[test]
    fn a_clean_receipt_has_no_forks_even_with_repeated_identical_rows() {
        let receipt = Receipt {
            accepted: vec![
                Acceptance {
                    work_id: "work-1".to_owned(),
                    title: "The Storm".to_owned(),
                    recipient: "https://a.example".to_owned(),
                },
                Acceptance {
                    work_id: "work-1".to_owned(),
                    title: "The Storm".to_owned(),
                    recipient: "https://a.example".to_owned(),
                },
            ],
            declined: Vec::new(),
        };
        assert!(
            receipt.forks().is_empty(),
            "one recipient recorded twice is one home, not a fork"
        );
    }
}
