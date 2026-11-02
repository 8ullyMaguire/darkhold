//! The baseline ladder: every rung a recommender is placed against, on one split.
//!
//! A score answers nothing on its own. `0.09 f1@20` is excellent against a popularity floor of
//! `0.036` and unremarkable against a study that reported `0.20`. So the unit of a verdict here is
//! the **ladder**, not the score: every rung is scored on the same readers, the same judgements and
//! the same cut, and a challenger ships only against the rung that carries the bar (popularity).
//!
//! The doctrine is spec §12.7a and §24.4; the plan is
//! `docs/plans/m64-16-offline-pre-registration-harness.md`; the reference numbers that motivate the
//! rungs are the vendored study under `docs/wiki/`.
//!
//! # What it refuses
//!
//! - **A refused rung is not a zero.** A rung whose data does not exist reports `refused: Some(..)`
//!   and `None` for every metric. Scoring it `0.0` would put a missing table at the bottom of a
//!   table and invite the reading "the tag engine is bad" when the truth is "no engaged row carries
//!   a tag".
//! - **A reader with no judgements is not a perfect run.** They are excluded and counted
//!   (`scored` beside `readers`), never averaged in as `1.0`.
//! - **Coverage is part of every score.** A rung that recommends the same twenty works to everybody
//!   scores well and is unusable; the distinct-work count and the catalogue share are printed beside
//!   the metric, and the verdict reads them as guardrails.
//! - **It does not compare a rung to another corpus's number.** The published study's figures are
//!   other readers on other works; `0.2025` is not this instance's target and this module has no
//!   constant that could pretend otherwise.

use std::collections::{BTreeMap, HashMap};

use crate::eval::{average_precision_at, f1_at, ndcg_at, precision_at, recall_at, Judgement};

/// The cut every rung is scored at.
///
/// Fixed and printed rather than configurable: an F1 at a moving `k` cannot be compared with the
/// previous run, and comparing runs is the whole job.
pub const K: usize = 20;

/// One engine's answer for one reader.
#[derive(Debug, Clone, PartialEq)]
pub struct Rung {
    /// The name the report prints — `random`, `popularity`, `tag_i2i`, …
    pub name: String,
    /// Work ids in the order the rung would serve them. Ignored when `refused` is set.
    pub ranking: Vec<String>,
    /// Why this rung could not be computed. `Some` means the rung is named and not scored.
    pub refused: Option<String>,
}

/// One reader: what they engaged with after the cutoff, and every rung's answer for them.
#[derive(Debug, Clone, PartialEq)]
pub struct Case {
    /// Stable identifier. Pseudonymised by the exporter, never an account id (spec §0.3).
    pub reader: String,
    /// The works this reader engaged with after the cutoff: the thing being predicted.
    pub judged: Vec<Judgement>,
    /// Every rung's ranking for this reader, matched to [`RungScore`] by name.
    pub rungs: Vec<Rung>,
}

/// What a challenger must clear. Pre-registered: written before the run, and the run cannot move it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bar {
    /// The challenger's `f1@k` must be at least this multiple of the popularity rung's.
    pub popularity_multiple: f64,
    /// And it must reach at least this share of the catalogue…
    pub min_catalogue_share: f64,
    /// …without putting more than this share of what it serves into one tag group.
    pub max_group_share: f64,
}

impl Bar {
    /// The proposed bar: `1.25×` popularity, `≥ 5 %` of the catalogue, no group above `40 %`.
    ///
    /// The multiple is a **proposal, not a measurement**. The published study's gap between
    /// popularity (`0.0364`) and a neighbourhood walk (`0.0933`) is 2.6×, so 1.25× is the smallest
    /// number that is clearly not noise while staying reachable on a thin corpus. An operator who
    /// disagrees changes the constant and says so; what must not happen is a run choosing its own
    /// bar after seeing the numbers.
    #[must_use]
    pub const fn proposed() -> Self {
        Self {
            popularity_multiple: 1.25,
            min_catalogue_share: 0.05,
            max_group_share: 0.40,
        }
    }
}

/// One rung's totals over every case.
#[derive(Debug, Clone, PartialEq)]
pub struct RungScore {
    /// The rung's name, as the report prints it.
    pub name: String,
    /// The refusal, when the rung could not be computed.
    pub refused: Option<String>,
    /// Readers this rung could be scored for.
    pub scored: usize,
    /// Mean `precision@k` over the scored readers.
    pub precision_at_k: Option<f64>,
    /// Mean `recall@k`.
    pub recall_at_k: Option<f64>,
    /// Mean `F1@k` — the ladder's headline.
    pub f1_at_k: Option<f64>,
    /// Mean average precision at `k`.
    pub map_at_k: Option<f64>,
    /// Mean `nDCG@10`, kept because the parity harness already scores in it.
    pub ndcg_at_10: Option<f64>,
    /// Distinct works across every case's ranking — the anti-collapse measure.
    pub distinct_works: usize,
    /// `distinct_works / catalogue_size`.
    pub catalogue_share: f64,
    /// The largest share of one case's served slots held by a single group, or `None` when no
    /// group key was supplied.
    pub largest_group_share: Option<f64>,
}

impl RungScore {
    /// Whether this rung produced numbers at all.
    #[must_use]
    pub fn is_scored(&self) -> bool {
        self.refused.is_none() && self.scored > 0
    }
}

/// The verdict on one named challenger.
#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    /// The rung under test.
    pub challenger: String,
    /// Its `f1@k` divided by the popularity rung's, or `None` when either side is unmeasured.
    pub f1_ratio: Option<f64>,
    /// Whether every limb of the conjunction passed.
    pub shipped: bool,
    /// The sentence a human reads. It names the reason either way, because a "no" without a named
    /// limb is how a bar becomes a negotiation.
    pub reason: String,
}

/// The whole run: every rung's totals, the denominators, and the bar it was read against.
#[derive(Debug, Clone, PartialEq)]
pub struct LadderReport {
    /// The cut every rung was scored at.
    pub k: usize,
    /// Works the catalogue holds, or `0` when the caller did not know it.
    pub catalogue_size: usize,
    /// Readers in the run, including those with nothing judged.
    pub readers: usize,
    /// Readers the rungs could actually be scored for.
    pub scored: usize,
    /// Totals per rung, in the order the rungs were first seen.
    pub rungs: Vec<RungScore>,
    /// The bar the verdict is read against.
    pub bar: Bar,
}

/// Mean of the values that exist, or `None` when none do.
fn mean(values: impl Iterator<Item = f64>) -> Option<f64> {
    let collected: Vec<f64> = values.collect();
    if collected.is_empty() {
        return None;
    }
    Some(collected.iter().sum::<f64>() / collected.len() as f64)
}

/// The most-common group in one case's served slots, as a share of the slots served.
///
/// The denominator is **what the rung served**, not `k`: a rung that served two works and both were
/// Harry Potter is 100 % concentrated, and reporting that as `10 %` because the page has twenty slots
/// would hide exactly the collapse this guardrail exists to catch.
fn group_share(ranking: &[String], groups: &HashMap<String, String>) -> Option<f64> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut served = 0_usize;
    for work in ranking.iter().take(K) {
        if let Some(group) = groups.get(work) {
            *counts.entry(group.as_str()).or_default() += 1;
            served += 1;
        }
    }
    if served == 0 {
        return None;
    }
    let largest = counts.values().copied().max().unwrap_or(0);
    Some(largest as f64 / served as f64)
}

impl LadderReport {
    /// Score every case, every rung, and average.
    ///
    /// `groups` maps a work id to its fandom group (spec §11.8). An empty map means no group
    /// guardrail is computed — `None`, never `0.0`, because a concentration of zero is a claim and
    /// "nobody supplied the groups" is a gap.
    #[must_use]
    pub fn build(
        cases: &[Case],
        catalogue_size: usize,
        groups: &HashMap<String, String>,
        bar: Bar,
    ) -> Self {
        // Rung names in first-seen order, so the report reads like the ladder it describes.
        let mut names: Vec<String> = Vec::new();
        for case in cases {
            for rung in &case.rungs {
                if !names.iter().any(|name| name == &rung.name) {
                    names.push(rung.name.clone());
                }
            }
        }

        let scored = cases
            .iter()
            .filter(|case| case.judged.iter().any(|j| j.grade > 0))
            .count();

        let mut rungs = Vec::with_capacity(names.len());
        for name in names {
            let mut precision = Vec::new();
            let mut recall = Vec::new();
            let mut f1 = Vec::new();
            let mut map = Vec::new();
            let mut ndcg = Vec::new();
            let mut refused: Option<String> = None;
            let mut distinct: BTreeMap<&str, ()> = BTreeMap::new();
            let mut shares: Vec<f64> = Vec::new();

            for case in cases {
                let Some(rung) = case.rungs.iter().find(|r| r.name == name) else {
                    continue;
                };
                if let Some(reason) = &rung.refused {
                    refused = Some(reason.clone());
                    continue;
                }
                for work in rung.ranking.iter().take(K) {
                    distinct.insert(work.as_str(), ());
                }
                if let Some(share) = group_share(&rung.ranking, groups) {
                    shares.push(share);
                }
                if let Some(value) = f1_at(&rung.ranking, &case.judged, K) {
                    f1.push(value);
                    precision.push(precision_at(&rung.ranking, &case.judged, K));
                }
                if let Some(value) = recall_at(&rung.ranking, &case.judged, K) {
                    recall.push(value);
                }
                if let Some(value) = average_precision_at(&rung.ranking, &case.judged, K) {
                    map.push(value);
                }
                if let Some(value) = ndcg_at(&rung.ranking, &case.judged, 10) {
                    ndcg.push(value);
                }
            }

            let distinct_works = distinct.len();
            let f1_count = f1.len();
            let was_refused = refused.is_some();
            // A refused rung reports NO numbers, not zeroes: see the module docs.
            let (precision_at_k, recall_at_k, f1_at_k, map_at_k, ndcg_at_10, share) = if was_refused
            {
                (None, None, None, None, None, 0.0)
            } else {
                (
                    mean(precision.into_iter()),
                    mean(recall.into_iter()),
                    mean(f1.into_iter()),
                    mean(map.into_iter()),
                    mean(ndcg.into_iter()),
                    if catalogue_size == 0 {
                        0.0
                    } else {
                        distinct_works as f64 / catalogue_size as f64
                    },
                )
            };

            rungs.push(RungScore {
                name,
                refused,
                scored: if was_refused { 0 } else { f1_count },
                precision_at_k,
                recall_at_k,
                f1_at_k,
                map_at_k,
                ndcg_at_10,
                distinct_works,
                catalogue_share: share,
                largest_group_share: shares.into_iter().max_by(f64::total_cmp),
            });
        }

        Self {
            k: K,
            catalogue_size,
            readers: cases.len(),
            scored,
            rungs,
            bar,
        }
    }

    /// One rung's totals, by name.
    #[must_use]
    pub fn rung(&self, name: &str) -> Option<&RungScore> {
        self.rungs.iter().find(|rung| rung.name == name)
    }

    /// Read the conjunction for one challenger against the popularity rung.
    ///
    /// Three limbs, and every one of them is a way a good number can hide a bad engine: the ratio
    /// (is it better than the floor), the coverage (does it reach the catalogue), the concentration
    /// (is it the same twenty works for everybody). The reason names the failing limb.
    #[must_use]
    pub fn verdict(&self, challenger: &str) -> Verdict {
        let Some(candidate) = self.rung(challenger) else {
            return Verdict {
                challenger: challenger.to_string(),
                f1_ratio: None,
                shipped: false,
                reason: format!("no rung named {challenger} in this run"),
            };
        };
        if !candidate.is_scored() {
            let reason = candidate
                .refused
                .clone()
                .unwrap_or_else(|| "the rung produced no scored reader".to_string());
            return Verdict {
                challenger: challenger.to_string(),
                f1_ratio: None,
                shipped: false,
                reason: format!("{challenger} was not measured: {reason}"),
            };
        }
        let bar = self.rung("popularity").filter(|rung| rung.is_scored());
        let ratio = match (candidate.f1_at_k, bar.and_then(|rung| rung.f1_at_k)) {
            (Some(candidate), Some(floor)) if floor > 0.0 => Some(candidate / floor),
            _ => None,
        };

        let mut reason = String::new();
        let mut shipped = true;

        match ratio {
            Some(ratio) if ratio >= self.bar.popularity_multiple => {}
            Some(ratio) => {
                shipped = false;
                reason = format!(
                    "f1@{} is {ratio:.2}× popularity, below the {:.2}× bar",
                    self.k, self.bar.popularity_multiple
                );
            }
            None => {
                shipped = false;
                reason = format!(
                    "no popularity rung to compare against, so f1@{} measures nothing",
                    self.k
                );
            }
        }

        if shipped && candidate.catalogue_share < self.bar.min_catalogue_share {
            shipped = false;
            reason = format!(
                "coverage {:.1} % is below the {:.1} % floor",
                candidate.catalogue_share * 100.0,
                self.bar.min_catalogue_share * 100.0
            );
        }

        if shipped {
            match candidate.largest_group_share {
                Some(share) if share > self.bar.max_group_share => {
                    shipped = false;
                    reason = format!(
                        "one group holds {:.1} % of a page, above the {:.1} % ceiling",
                        share * 100.0,
                        self.bar.max_group_share * 100.0
                    );
                }
                _ => {}
            }
        }

        if shipped {
            reason = match ratio {
                Some(ratio) => format!(
                    "{ratio:.2}× popularity at {:.1} % coverage, no guardrail breached",
                    candidate.catalogue_share * 100.0
                ),
                None => "measured".to_string(),
            };
        }

        Verdict {
            challenger: challenger.to_string(),
            f1_ratio: ratio,
            shipped,
            reason,
        }
    }

    /// The lines a harness prints, so every number arrives with its denominator.
    #[must_use]
    pub fn to_text(&self, challenger: &str) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "k={} · catalogue {} · readers {} ({} scored, {} unjudged)\n\n",
            self.k,
            self.catalogue_size,
            self.readers,
            self.scored,
            self.readers.saturating_sub(self.scored),
        ));
        out.push_str(&format!(
            "{:<20}{:>8}{:>9}{:>9}{:>10}{:>8}{:>11}  {}\n",
            "rung", "f1@k", "map@k", "nDCG@10", "distinct", "share", "max-group", "status"
        ));
        for rung in &self.rungs {
            let number = |value: Option<f64>| match value {
                Some(value) => format!("{value:.4}"),
                None => "—".to_string(),
            };
            let status = match &rung.refused {
                Some(reason) => format!("REFUSED {reason}"),
                None if rung.scored == 0 => "scored 0 readers".to_string(),
                None => "scored".to_string(),
            };
            out.push_str(&format!(
                "{:<20}{:>8}{:>9}{:>9}{:>10}{:>7.1}%{:>11}  {}\n",
                rung.name,
                number(rung.f1_at_k),
                number(rung.map_at_k),
                number(rung.ndcg_at_10),
                rung.distinct_works,
                rung.catalogue_share * 100.0,
                match rung.largest_group_share {
                    Some(share) => format!("{:.1}%", share * 100.0),
                    None => "—".to_string(),
                },
                status,
            ));
        }
        let verdict = self.verdict(challenger);
        out.push_str(&format!(
            "\nverdict: {} {} — {}\n",
            verdict.challenger,
            if verdict.shipped {
                "SHIPPED"
            } else {
                "NOT SHIPPED"
            },
            verdict.reason
        ));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::{Bar, Case, LadderReport, Rung, K};
    use crate::eval::{average_precision_at, f1_at, Judgement};
    use std::collections::HashMap;

    fn judged(pairs: &[(&str, u32)]) -> Vec<Judgement> {
        pairs
            .iter()
            .map(|(id, grade)| Judgement {
                work_id: (*id).to_string(),
                grade: *grade,
            })
            .collect()
    }

    fn ranking(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| (*id).to_string()).collect()
    }

    fn rung(name: &str, ids: &[&str]) -> Rung {
        Rung {
            name: name.to_string(),
            ranking: ranking(ids),
            refused: None,
        }
    }

    fn refused(name: &str, why: &str) -> Rung {
        Rung {
            name: name.to_string(),
            ranking: Vec::new(),
            refused: Some(why.to_string()),
        }
    }

    fn no_groups() -> HashMap<String, String> {
        HashMap::new()
    }

    #[test]
    fn a_refused_rung_is_not_a_zero() {
        let cases = vec![Case {
            reader: "r1".into(),
            judged: judged(&[("a", 1)]),
            rungs: vec![
                rung("popularity", &["a", "b"]),
                refused("tag_i2i", "0 of 1247 engaged rows carry a work_tag"),
            ],
        }];
        let report = LadderReport::build(&cases, 100, &no_groups(), Bar::proposed());
        let tag = report
            .rung("tag_i2i")
            .expect("the refused rung is still named");
        assert_eq!(tag.f1_at_k, None, "a refusal must not report 0.0");
        assert_eq!(tag.scored, 0);
        assert!(!tag.is_scored());
        assert!(tag.refused.is_some());
        assert!(
            report.to_text("tag_i2i").contains("REFUSED 0 of 1247"),
            "the reason must reach the printed line:\n{}",
            report.to_text("tag_i2i")
        );
        // The popularity rung in the same run is scored, so the table is not uniformly empty.
        assert!(report.rung("popularity").unwrap().is_scored());
    }

    #[test]
    fn the_bar_is_a_conjunction() {
        // The challenger finds both judged works; the floor finds one. That is 2x the ratio, and
        // 2 works out of a 100-work catalogue is 2 % coverage — so the ratio passes and coverage
        // fails. This is the test that stops a winning number from hiding a collapsed engine.
        let cases: Vec<Case> = (0..2)
            .map(|i| Case {
                reader: format!("r{i}"),
                judged: judged(&[("a", 1), ("b", 1)]),
                rungs: vec![rung("popularity", &["a"]), rung("challenger", &["a", "b"])],
            })
            .collect();

        let report = LadderReport::build(&cases, 100, &no_groups(), Bar::proposed());
        let verdict = report.verdict("challenger");
        assert!(
            verdict.f1_ratio.unwrap() > 1.25,
            "the ratio limb passed: {:?}",
            verdict.f1_ratio
        );
        assert!(!verdict.shipped);
        assert!(verdict.reason.contains("coverage"), "{}", verdict.reason);

        // The same run on a catalogue where two works is 10 % coverage clears the bar, which
        // proves the limb that failed above was coverage and not something else.
        let report = LadderReport::build(&cases, 20, &no_groups(), Bar::proposed());
        assert!(report.verdict("challenger").shipped);
    }

    #[test]
    fn the_ratio_is_measured_against_popularity() {
        // A floor that finds nothing has no ratio to compute, and the verdict says so rather than
        // dividing by zero or by an absent rung.
        let cases = vec![Case {
            reader: "r1".into(),
            judged: judged(&[("a", 1)]),
            rungs: vec![rung("popularity", &["z"]), rung("challenger", &["a"])],
        }];
        let report = LadderReport::build(&cases, 2, &no_groups(), Bar::proposed());
        let verdict = report.verdict("challenger");
        assert_eq!(verdict.f1_ratio, None);
        assert!(!verdict.shipped);
        assert!(
            verdict.reason.contains("no popularity rung"),
            "{}",
            verdict.reason
        );

        // With a real floor the arithmetic is the ratio, and the verdict quotes it.
        let cases: Vec<Case> = (0..2)
            .map(|i| Case {
                reader: format!("r{i}"),
                judged: judged(&[("a", 1), ("b", 1)]),
                rungs: vec![rung("popularity", &["a"]), rung("challenger", &["a", "b"])],
            })
            .collect();
        let report = LadderReport::build(&cases, 2, &no_groups(), Bar::proposed());
        let verdict = report.verdict("challenger");
        let ratio = verdict.f1_ratio.expect("both rungs are scored");
        assert!((ratio - 2.0).abs() < 1e-12, "ratio = {ratio}");
        assert!(verdict.shipped, "{}", verdict.reason);
        assert!(
            verdict.reason.contains("2.00× popularity"),
            "{}",
            verdict.reason
        );
    }

    #[test]
    fn coverage_counts_distinct_works_not_slots() {
        // Ten readers, each served the same two works: distinct is 2, not 20.
        let cases: Vec<Case> = (0..10)
            .map(|i| Case {
                reader: format!("r{i}"),
                judged: judged(&[("a", 1)]),
                rungs: vec![rung("popularity", &["a", "b"])],
            })
            .collect();
        let report = LadderReport::build(&cases, 100, &no_groups(), Bar::proposed());
        let score = report.rung("popularity").unwrap();
        assert_eq!(score.distinct_works, 2);
        assert!((score.catalogue_share - 0.02).abs() < 1e-12);
    }

    #[test]
    fn a_reader_without_judgements_is_counted_not_averaged() {
        let cases = vec![
            Case {
                reader: "scored".into(),
                judged: judged(&[("a", 1)]),
                rungs: vec![rung("popularity", &["a"])],
            },
            Case {
                reader: "unjudged".into(),
                judged: Vec::new(),
                rungs: vec![rung("popularity", &["a"])],
            },
        ];
        let report = LadderReport::build(&cases, 1, &no_groups(), Bar::proposed());
        assert_eq!(report.readers, 2);
        assert_eq!(report.scored, 1);
        let score = report.rung("popularity").unwrap();
        assert_eq!(score.scored, 1, "an unjudged reader is not scored");
        // One hit in twenty slots against a single relevant work: 2*0.05*1.0/1.05. If the unjudged
        // reader were averaged in as 0.0 the mean would be half this.
        let expected = 2.0 * 0.05 * 1.0 / 1.05;
        assert!((score.f1_at_k.unwrap() - expected).abs() < 1e-12);
        assert!(report
            .to_text("popularity")
            .contains("2 (1 scored, 1 unjudged)"));
    }

    #[test]
    fn a_group_collapse_is_a_guardrail_not_a_zero() {
        let mut groups = HashMap::new();
        for work in ["a", "b", "c"] {
            groups.insert(work.to_string(), "harry potter".to_string());
        }
        let cases: Vec<Case> = (0..2)
            .map(|i| Case {
                reader: format!("r{i}"),
                judged: judged(&[("a", 1), ("b", 1)]),
                rungs: vec![rung("popularity", &["a"]), rung("challenger", &["a", "b"])],
            })
            .collect();
        let report = LadderReport::build(&cases, 3, &groups, Bar::proposed());
        let verdict = report.verdict("challenger");
        assert!(
            verdict.f1_ratio.unwrap() > 1.25,
            "the ratio limb passed, so the failure below is the group ceiling"
        );
        assert_eq!(
            report.rung("challenger").unwrap().largest_group_share,
            Some(1.0),
            "every served work is one group"
        );
        assert!(!verdict.shipped);
        assert!(verdict.reason.contains("one group"), "{}", verdict.reason);

        // With no group map the same run has no concentration claim at all: None, never 0.0.
        let report = LadderReport::build(&cases, 3, &no_groups(), Bar::proposed());
        assert_eq!(report.rung("challenger").unwrap().largest_group_share, None);
        assert!(report.verdict("challenger").shipped);
    }

    #[test]
    fn an_unknown_challenger_names_itself() {
        let cases = vec![Case {
            reader: "r1".into(),
            judged: judged(&[("a", 1)]),
            rungs: vec![rung("popularity", &["a"])],
        }];
        let report = LadderReport::build(&cases, 1, &no_groups(), Bar::proposed());
        let verdict = report.verdict("no-such-engine");
        assert!(!verdict.shipped);
        assert!(verdict.reason.contains("no rung named no-such-engine"));
    }

    #[test]
    fn the_cut_and_the_metrics_are_pinned() {
        // A guard on the two numbers every other rung is compared at.
        assert_eq!(K, 20);
        let judged = judged(&[("a", 1)]);
        let ranked = ranking(&["a"]);
        assert!((f1_at(&ranked, &judged, K).unwrap() - 2.0 * 0.05 / 1.05).abs() < 1e-12);
        assert!((average_precision_at(&ranked, &judged, K).unwrap() - 1.0).abs() < 1e-12);
    }
}
