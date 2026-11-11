//! An import source's intake policy: what it may bring in, and what it queues (M69-23, Suki §8.4/§11.4).
//!
//! The row asked for a *Saved Filter* carried by an import source. Measured before building it: a saved view's document is
//! either a `LibraryQuery` — `shelves`, `tags`, `statuses`, `source`, `updated_since`, `sort`, where the first three are
//! **per-account facts about the reader's own shelf** — or a catalogue query *string* for `GET /catalogue`. Neither can be
//! an intake policy: an arriving work has no shelf, no reading status and no "updated since" for this instance, so a policy
//! written in that language would parse, evaluate, and mean nothing. The census verdict is therefore BUILD, and the thing it
//! builds is small on purpose.
//!
//! # The shape, and why it is ordered rules rather than one expression
//!
//! An operator has to be able to *read* the policy and predict it, which is why this is data (`serde`) with no code in it and
//! why the decision names the rule that made it. Three properties are load-bearing:
//!
//! * **First match wins.** Two rules that overlap must not both fire, and the operator's ordering has to mean something.
//! * **No match queues, never admits.** The default is the cautious one: an unclassified arrival is something a person looks
//!   at. A policy that admitted by default would make a missing rule indistinguishable from a deliberate one.
//! * **Every decision is explainable.** [`IntakeDecision`] carries the rule index and a sentence, so a dry run can show what
//!   the policy would do to a candidate — which is the row's actual claim: "the difference between a policy an operator can
//!   inspect and one they have to trust".

use serde::{Deserialize, Serialize};

/// What an arrival is, as far as the policy can see. Deliberately a flat, small struct: a policy that could read anything
/// would be a policy nobody can predict from the source's configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntakeCandidate {
    /// The source host the arrival came from, lowercased (`archiveofourown.org`).
    pub host: String,
    /// The declared language, as the importer reports it (`en`, `ja`, or empty when undeclared).
    #[serde(default)]
    pub language: String,
    /// Word count, `0` when the source does not say.
    #[serde(default)]
    pub words: i64,
    /// Tags, lowercased and deduplicated by the caller.
    #[serde(default)]
    pub tags: Vec<String>,
    /// The title, as the source gave it.
    #[serde(default)]
    pub title: String,
    /// Whether the source marks the work complete.
    #[serde(default)]
    pub complete: bool,
}

/// One condition on a candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum Condition {
    /// The host is one of these (exact, lowercased).
    HostIn(Vec<String>),
    /// The host is none of these.
    HostNotIn(Vec<String>),
    /// The language is one of these.
    LanguageIn(Vec<String>),
    /// At least this many words.
    WordsAtLeast(i64),
    /// At most this many words.
    WordsAtMost(i64),
    /// The arrival carries at least one of these tags.
    TagAny(Vec<String>),
    /// The arrival carries all of these tags.
    TagAll(Vec<String>),
    /// The title contains one of these substrings (case-insensitive).
    TitleContains(Vec<String>),
    /// The title contains none of these substrings (case-insensitive).
    TitleExcludes(Vec<String>),
    /// The source marks it complete.
    Complete,
}

impl Condition {
    /// Whether the candidate satisfies this condition.
    #[must_use]
    pub fn holds(&self, candidate: &IntakeCandidate) -> bool {
        let lower_title = candidate.title.to_lowercase();
        match self {
            Self::HostIn(hosts) => hosts.iter().any(|host| host == &candidate.host),
            Self::HostNotIn(hosts) => !hosts.iter().any(|host| host == &candidate.host),
            Self::LanguageIn(languages) => {
                languages.iter().any(|language| language == &candidate.language)
            }
            Self::WordsAtLeast(least) => candidate.words >= *least,
            Self::WordsAtMost(most) => candidate.words <= *most,
            Self::TagAny(wanted) => wanted
                .iter()
                .any(|tag| candidate.tags.iter().any(|have| have == tag)),
            Self::TagAll(wanted) => wanted
                .iter()
                .all(|tag| candidate.tags.iter().any(|have| have == tag)),
            Self::TitleContains(needles) => needles
                .iter()
                .any(|needle| lower_title.contains(&needle.to_lowercase())),
            Self::TitleExcludes(needles) => !needles
                .iter()
                .any(|needle| lower_title.contains(&needle.to_lowercase())),
            Self::Complete => candidate.complete,
        }
    }

    /// One sentence, for an operator reading a dry run.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::HostIn(hosts) => format!("host is one of {}", hosts.join(", ")),
            Self::HostNotIn(hosts) => format!("host is none of {}", hosts.join(", ")),
            Self::LanguageIn(languages) => format!("language is one of {}", languages.join(", ")),
            Self::WordsAtLeast(least) => format!("at least {least} words"),
            Self::WordsAtMost(most) => format!("at most {most} words"),
            Self::TagAny(tags) => format!("any of the tags {}", tags.join(", ")),
            Self::TagAll(tags) => format!("all of the tags {}", tags.join(", ")),
            Self::TitleContains(needles) => format!("title contains {}", needles.join(", ")),
            Self::TitleExcludes(needles) => format!("title contains none of {}", needles.join(", ")),
            Self::Complete => "the source marks it complete".to_owned(),
        }
    }
}

/// What the policy does with an arrival it matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Bring it in: write the row, index it, no human in the path.
    BringIn,
    /// Queue it: the arrival is stored where an operator can decide, and nothing is published.
    Queue,
}

/// One rule: when every condition holds, the verdict applies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    /// The conditions, all of which must hold.
    pub when: Vec<Condition>,
    /// What happens when they do.
    pub then: Verdict,
}

/// An import source's policy: ordered rules, first match wins.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntakePolicy {
    /// The rules, in the operator's order.
    #[serde(default)]
    pub rules: Vec<Rule>,
}

impl IntakePolicy {
    /// Whether anybody has actually written a policy.
    ///
    /// **A different question from [`decide`], and the difference is the whole safety of this feature.** `decide` answers "what does
    /// this policy do", and for no rules it answers *admit nothing* — correct for a policy, catastrophic for a default: `{}` is what
    /// every source carries until an operator writes one, so an importer that consulted `decide` unconditionally would hold every
    /// arrival on the instance. The importer asks this first, and an unconfigured source keeps behaving exactly as it did before the
    /// policy existed.
    #[must_use]
    pub fn is_configured(&self) -> bool {
        !self.rules.is_empty()
    }
}

/// What the policy decided, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntakeDecision {
    /// Bring the arrival in, by this rule.
    BringIn {
        /// The rule that matched, zero-based.
        rule: usize,
        /// The rule, in words.
        because: String,
    },
    /// Queue the arrival, by this rule.
    Queue {
        /// The rule that matched, zero-based.
        rule: usize,
        /// The rule, in words.
        because: String,
    },
    /// No rule matched, so a person looks at it.
    QueueUnmatched,
}

impl IntakeDecision {
    /// Whether the arrival may be written without a human.
    #[must_use]
    pub fn brings_in(&self) -> bool {
        matches!(self, Self::BringIn { .. })
    }

    /// One sentence for an operator.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::BringIn { rule, because } => {
                format!("bring in: rule {rule} matched ({because})")
            }
            Self::Queue { rule, because } => {
                format!("queue: rule {rule} matched ({because})")
            }
            Self::QueueUnmatched => {
                "queue: no rule matched, so this arrival needs a person".to_owned()
            }
        }
    }
}

/// Decide what an import source does with one arrival.
///
/// First match wins; no match queues. Both halves are the row: "what this source may bring in" is the `BringIn` rules, and
/// "what it queues" is everything else — including, deliberately, the arrivals nobody wrote a rule for.
#[must_use]
pub fn decide(policy: &IntakePolicy, candidate: &IntakeCandidate) -> IntakeDecision {
    for (index, rule) in policy.rules.iter().enumerate() {
        if rule.when.iter().all(|condition| condition.holds(candidate)) {
            let because = rule
                .when
                .iter()
                .map(Condition::describe)
                .collect::<Vec<_>>()
                .join(" and ");
            return match rule.then {
                Verdict::BringIn => IntakeDecision::BringIn {
                    rule: index,
                    because,
                },
                Verdict::Queue => IntakeDecision::Queue {
                    rule: index,
                    because,
                },
            };
        }
    }
    IntakeDecision::QueueUnmatched
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> IntakePolicy {
        IntakePolicy {
            rules: vec![
                Rule {
                    when: vec![
                        Condition::HostIn(vec!["archiveofourown.org".to_owned()]),
                        Condition::LanguageIn(vec!["en".to_owned()]),
                        Condition::WordsAtLeast(1_000),
                        Condition::TitleExcludes(vec!["[podfic]".to_owned()]),
                    ],
                    then: Verdict::BringIn,
                },
                Rule {
                    when: vec![Condition::TagAll(vec!["dead dove".to_owned()])],
                    then: Verdict::Queue,
                },
            ],
        }
    }

    fn candidate(host: &str, language: &str, words: i64, tags: &[&str], title: &str) -> IntakeCandidate {
        IntakeCandidate {
            host: host.to_owned(),
            language: language.to_owned(),
            words,
            tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
            title: title.to_owned(),
            complete: false,
        }
    }

    #[test]
    fn a_matching_rule_brings_the_arrival_in_and_names_itself() {
        let decision = decide(
            &policy(),
            &candidate("archiveofourown.org", "en", 4_000, &[], "A Long Story"),
        );
        assert!(decision.brings_in(), "{decision:?}");
        match decision {
            IntakeDecision::BringIn { rule, because } => {
                assert_eq!(rule, 0);
                assert!(because.contains("at least 1000 words"), "{because}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_failed_condition_falls_through_rather_than_being_ignored() {
        // The word count is the only failing clause; the arrival must not be admitted on the others.
        let decision = decide(
            &policy(),
            &candidate("archiveofourown.org", "en", 40, &[], "A Fragment"),
        );
        assert_eq!(decision, IntakeDecision::QueueUnmatched);
    }

    #[test]
    fn an_unmatched_arrival_is_queued_not_admitted() {
        let decision = decide(&policy(), &candidate("example.invalid", "fr", 9_000, &[], "X"));
        assert!(!decision.brings_in(), "the cautious default is a person: {decision:?}");
        assert_eq!(decision, IntakeDecision::QueueUnmatched);
    }

    #[test]
    fn an_empty_policy_admits_nothing() {
        let decision = decide(&IntakePolicy::default(), &candidate("ao3", "en", 1, &[], "Any"));
        assert!(!decision.brings_in(), "no rules is not 'everything goes'");
    }

    #[test]
    fn first_match_wins_so_an_exclusion_above_a_take_rule_is_respected() {
        let policy = IntakePolicy {
            rules: vec![
                Rule {
                    when: vec![Condition::TagAll(vec!["dead dove".to_owned()])],
                    then: Verdict::Queue,
                },
                Rule {
                    when: vec![Condition::HostIn(vec!["ao3".to_owned()])],
                    then: Verdict::BringIn,
                },
            ],
        };
        let tagged = decide(&policy, &candidate("ao3", "en", 5_000, &["dead dove"], "Dark"));
        assert_eq!(
            tagged,
            IntakeDecision::Queue {
                rule: 0,
                because: "all of the tags dead dove".to_owned()
            },
            "the queue rule is above the take rule, and ordering is the operator's"
        );
        let plain = decide(&policy, &candidate("ao3", "en", 5_000, &[], "Light"));
        assert!(plain.brings_in());
    }

    #[test]
    fn tag_any_and_title_filters_do_what_they_say() {
        let any = Condition::TagAny(vec!["slow burn".to_owned(), "fluff".to_owned()]);
        assert!(any.holds(&candidate("h", "en", 1, &["fluff"], "t")));
        assert!(!any.holds(&candidate("h", "en", 1, &["angst"], "t")));

        let excluded = Condition::TitleExcludes(vec!["[podfic]".to_owned()]);
        assert!(!excluded.holds(&candidate("h", "en", 1, &[], "[Podfic] Anything")));
        assert!(excluded.holds(&candidate("h", "en", 1, &[], "Anything Else")));
    }

    #[test]
    fn a_policy_with_no_rules_is_not_a_policy() {
        // The distinction the importer depends on: `decide` says "admit nothing" for an empty policy, which is right for a policy and
        // wrong for a default — so "is there a policy at all" has to be askable on its own, and the two answers must disagree here.
        let empty = IntakePolicy::default();
        assert!(!empty.is_configured(), "no rules is not a configured source");
        assert_eq!(
            decide(&empty, &candidate("ao3", "en", 5_000, &[], "T")),
            IntakeDecision::QueueUnmatched,
            "and the same policy still admits nothing if someone consults it directly"
        );
        assert!(policy().is_configured(), "two rules is a policy");
    }

    #[test]
    fn every_decision_can_be_explained_to_a_person() {
        let decision = decide(
            &policy(),
            &candidate("archiveofourown.org", "en", 4_000, &[], "A Long Story"),
        );
        assert!(
            decision.describe().starts_with("bring in: rule 0 matched"),
            "{}",
            decision.describe()
        );
        assert!(IntakeDecision::QueueUnmatched.describe().contains("needs a person"));
    }
}
