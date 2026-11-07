//! §21.5's review workflow, as a state machine (M16-07).
//!
//! ```text
//! submitted → automated checks → permission review → security review → quorum approval → published
//!                                                                   ↘ rejected
//! published → revoked (emergency, with rollback to the previous published version)
//! ```
//!
//! # Why this is a pure module
//!
//! The rules are the product here, and they are rules about *who may say what, when*. Putting them in SQL or in a handler means
//! they can only be tested through a database and an HTTP client, and the first thing that happens to a rule like that is that somebody
//! adds a route that skips a stage because the queue was inconvenient. This module has no I/O: a transition is a value in, a value out,
//! and the store records what this decided.
//!
//! # The four rules that matter
//!
//! * **No stage may be skipped.** A package cannot be published from `submitted`, and automated checks cannot be waived into
//!   `published`. The order is the review.
//! * **The submitter never reviews their own package.** Not for a single stage, and not as one of the quorum approvals — self-review is
//!   the cheapest possible way to defeat a review workflow, and it is the one an author will always find a reason to ask for.
//! * **Permission and security review are different people.** The permission reviewer decides what the package is allowed to *have*; the
//!   security reviewer decides whether it may be *trusted* with it. One person doing both is one opinion with two headings.
//! * **A paid package follows the same path as a free one.** There is no field in this module that could be set to skip review, because
//!   a paid status that skipped security review would be a way to buy the review — so the absence is the guarantee.

use std::collections::BTreeSet;

/// Where a submission is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Stage {
    /// The author has submitted the manifest and the package bytes.
    #[default]
    Submitted,
    /// The automated checks are running or have run: the sandbox exercised the package and its imports were compared with the declared
    /// capabilities.
    AutomatedChecks,
    /// A human decides whether the declared capabilities are the ones the package needs, and nothing more.
    PermissionReview,
    /// A different human decides whether the package is safe to trust with those capabilities.
    SecurityReview,
    /// Independent approvers, reaching [`QUORUM`].
    Quorum,
    /// Published: a manifest row exists for readers to grant.
    Published,
    /// Refused, with a reason. Terminal.
    Rejected,
    /// Was published, is not now. Terminal for that version; the previous version is what readers have.
    Revoked,
}

impl Stage {
    /// The wire name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Submitted => "submitted",
            Self::AutomatedChecks => "automated_checks",
            Self::PermissionReview => "permission_review",
            Self::SecurityReview => "security_review",
            Self::Quorum => "quorum",
            Self::Published => "published",
            Self::Rejected => "rejected",
            Self::Revoked => "revoked",
        }
    }

    /// Parse the wire name.
    ///
    /// Not `FromStr`: the wire names are this workflow's own spelling (`permission_review`, not `PermissionReview`), and inventing a
    /// standard trait for a private vocabulary invites a caller to expect `to_string` to round-trip through it. It does not — `as_str`
    /// is the other half, and both are here so the pair is obvious.
    #[must_use]
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(text: &str) -> Option<Self> {
        Some(match text {
            "submitted" => Self::Submitted,
            "automated_checks" => Self::AutomatedChecks,
            "permission_review" => Self::PermissionReview,
            "security_review" => Self::SecurityReview,
            "quorum" => Self::Quorum,
            "published" => Self::Published,
            "rejected" => Self::Rejected,
            "revoked" => Self::Revoked,
            _ => return None,
        })
    }

    /// Whether nothing more can happen to this submission.
    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Rejected | Self::Revoked)
    }

    /// The stage that follows, when there is exactly one.
    #[must_use]
    pub fn next(self) -> Option<Self> {
        match self {
            Self::Submitted => Some(Self::AutomatedChecks),
            Self::AutomatedChecks => Some(Self::PermissionReview),
            Self::PermissionReview => Some(Self::SecurityReview),
            Self::SecurityReview => Some(Self::Quorum),
            Self::Quorum => Some(Self::Published),
            Self::Published | Self::Rejected | Self::Revoked => None,
        }
    }
}

/// How many independent approvals publication needs.
///
/// Two rather than one: one approver is a rubber stamp regardless of what the workflow calls them, and §21.5 says *quorum* rather than
/// "an approver". Kept as a constant with the number in one place, so raising it is a decision rather than a search.
pub const QUORUM: usize = 2;

/// Why a transition was refused.
///
/// Each variant is a sentence a reviewer can act on. `StageSkipped` is deliberately not `InvalidState`: the difference between
/// "somebody tried to publish without review" and "somebody advanced an already-rejected submission" matters in an audit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// A stage was skipped.
    StageSkipped { from: Stage, to: Stage },
    /// The submission is finished.
    AlreadyDecided(Stage),
    /// The submission is the reviewer's own.
    SelfReview,
    /// The permission and security reviewers are the same person.
    ReviewersNotIndependent,
    /// The same approver was counted twice.
    DuplicateApproval(String),
    /// Fewer than [`QUORUM`] distinct approvals.
    QuorumNotReached { have: usize, need: usize },
    /// Automated checks have not passed, so no human stage may begin.
    ChecksFailed,
    /// A stage name this module does not know.
    UnknownStage(String),
}

impl Refusal {
    /// The sentence a caller may publish.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::StageSkipped { from, to } => format!(
                "a submission at `{}` cannot move to `{}`; the stages run in order and none may be skipped",
                from.as_str(),
                to.as_str()
            ),
            Self::AlreadyDecided(stage) => {
                format!("this submission is already `{}`", stage.as_str())
            }
            Self::SelfReview => "the submitter cannot review their own package".to_owned(),
            Self::ReviewersNotIndependent => {
                "permission review and security review must be two different people".to_owned()
            }
            Self::DuplicateApproval(who) => format!("{who} has already approved this submission"),
            Self::QuorumNotReached { have, need } => {
                format!("{have} independent approvals, {need} needed")
            }
            Self::ChecksFailed => {
                "the automated checks have not passed, so the human stages cannot begin".to_owned()
            }
            Self::UnknownStage(name) => format!("`{name}` is not a review stage"),
        }
    }
}

/// One automated check's outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    /// The check's name, as the report shows it (`sandbox.runs`, `imports.undeclared`).
    pub name: String,
    /// Whether it passed, and why not when it did not.
    pub outcome: Result<(), String>,
}

/// Everything the workflow knows about one submission.
#[derive(Debug, Clone, Default)]
pub struct Review {
    pub stage: Stage,
    /// The account that submitted the package. Never allowed to review it.
    pub submitter: String,
    /// Automated checks, in the order they ran.
    pub checks: Vec<Check>,
    /// The account that performed permission review.
    pub permission_reviewer: Option<String>,
    /// The account that performed security review.
    pub security_reviewer: Option<String>,
    /// Distinct approvers, in the order they approved.
    pub approvals: Vec<String>,
}

/// What a caller asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Record the automated check results. Requires every check present to have passed.
    RecordChecks,
    /// Move on to the next stage, naming who is doing it.
    Advance { actor: String },
    /// Approve at the quorum stage.
    Approve { actor: String },
    /// Refuse the submission.
    Reject { actor: String, reason: String },
    /// Pull a published version.
    Revoke { actor: String, reason: String },
}

impl Review {
    /// A review starts at `Submitted`.
    #[must_use]
    pub fn new(submitter: impl Into<String>) -> Self {
        Self {
            stage: Stage::Submitted,
            submitter: submitter.into(),
            ..Self::default()
        }
    }

    /// Whether every recorded check passed.
    #[must_use]
    pub fn checks_passed(&self) -> bool {
        !self.checks.is_empty() && self.checks.iter().all(|check| check.outcome.is_ok())
    }

    /// Whether a given account may review this submission at all.
    #[must_use]
    pub fn may_review(&self, actor: &str) -> bool {
        actor != self.submitter
    }

    /// Apply an action, returning the stage it moved to.
    ///
    /// The caller stores the returned stage and the new [`Review`]; nothing here mutates storage, and nothing here reads a clock.
    pub fn apply(&mut self, action: Action) -> Result<Stage, Refusal> {
        if self.stage.is_terminal() {
            return Err(Refusal::AlreadyDecided(self.stage));
        }
        // A published version is finished for every action except revocation: publishing is not a stage one walks out of, and rejecting a
        // version that readers already hold would leave its grants live with no package behind them.
        if self.stage == Stage::Published && !matches!(action, Action::Revoke { .. }) {
            return Err(Refusal::AlreadyDecided(self.stage));
        }
        match action {
            Action::RecordChecks => {
                if self.stage != Stage::Submitted {
                    return Err(Refusal::StageSkipped {
                        from: self.stage,
                        to: Stage::AutomatedChecks,
                    });
                }
                // The stage moves only when the checks speak for themselves. A reviewer who can wave a failure through has replaced the
                // check with their opinion, which is the thing automated checks exist to avoid.
                if !self.checks_passed() {
                    return Err(Refusal::ChecksFailed);
                }
                self.stage = Stage::AutomatedChecks;
                Ok(self.stage)
            }
            Action::Advance { actor } => {
                if !self.may_review(&actor) {
                    return Err(Refusal::SelfReview);
                }
                let next = self.stage.next().ok_or(Refusal::StageSkipped {
                    from: self.stage,
                    to: self.stage,
                })?;
                // `Quorum` is not a stage somebody advances through — it is reached by approvals — so advancing *out of* it is only
                // legal as the publish step, which `Approve` performs when the count is met.
                if self.stage == Stage::Quorum {
                    return Err(Refusal::StageSkipped {
                        from: self.stage,
                        to: next,
                    });
                }
                match (self.stage, next) {
                    (Stage::AutomatedChecks, Stage::PermissionReview)
                    | (Stage::PermissionReview, Stage::SecurityReview)
                    | (Stage::SecurityReview, Stage::Quorum) => {}
                    _ => {
                        return Err(Refusal::StageSkipped {
                            from: self.stage,
                            to: next,
                        })
                    }
                }
                // Record the reviewer at the stage that owns a name, and refuse a second reviewer who is the first one's twin.
                match self.stage {
                    Stage::AutomatedChecks => self.permission_reviewer = Some(actor),
                    Stage::PermissionReview => {
                        if self.permission_reviewer.as_deref() == Some(actor.as_str()) {
                            return Err(Refusal::ReviewersNotIndependent);
                        }
                        self.security_reviewer = Some(actor);
                    }
                    _ => {}
                }
                self.stage = next;
                Ok(self.stage)
            }
            Action::Approve { actor } => {
                if self.stage != Stage::Quorum {
                    return Err(Refusal::StageSkipped {
                        from: self.stage,
                        to: Stage::Published,
                    });
                }
                if !self.may_review(&actor) {
                    return Err(Refusal::SelfReview);
                }
                if self.approvals.iter().any(|approved| approved == &actor) {
                    return Err(Refusal::DuplicateApproval(actor));
                }
                self.approvals.push(actor);
                if self.approvals.len() < QUORUM {
                    return Err(Refusal::QuorumNotReached {
                        have: self.approvals.len(),
                        need: QUORUM,
                    });
                }
                self.stage = Stage::Published;
                Ok(self.stage)
            }
            Action::Reject { actor, reason } => {
                if !self.may_review(&actor) {
                    return Err(Refusal::SelfReview);
                }
                let _ = reason;
                self.stage = Stage::Rejected;
                Ok(self.stage)
            }
            Action::Revoke { actor, reason } => {
                if !self.may_review(&actor) {
                    return Err(Refusal::SelfReview);
                }
                if self.stage != Stage::Published {
                    return Err(Refusal::StageSkipped {
                        from: self.stage,
                        to: Stage::Revoked,
                    });
                }
                let _ = reason;
                self.stage = Stage::Revoked;
                Ok(self.stage)
            }
        }
    }

    /// Whether the package's permission set grew, which requires the reader to consent again.
    ///
    /// `true` when the new version asks for **anything** the granted set did not include. Fewer or equal capabilities need no new consent —
    /// a reader consented to a superset, and the subset is the thing they already said yes to. This is the only place the comparison is
    /// made, so "does this need renewed consent" cannot be answered two ways by two callers.
    #[must_use]
    pub fn needs_renewed_consent(granted: &BTreeSet<String>, requested: &BTreeSet<String>) -> bool {
        requested.difference(granted).next().is_some()
    }

    /// Which capabilities are new, so a consent prompt can name them.
    #[must_use]
    pub fn new_capabilities<'a>(
        granted: &'a BTreeSet<String>,
        requested: &'a BTreeSet<String>,
    ) -> Vec<&'a str> {
        requested.difference(granted).map(String::as_str).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn passing() -> Vec<Check> {
        vec![Check {
            name: "imports.declared".to_owned(),
            outcome: Ok(()),
        }]
    }

    fn at_quorum() -> Review {
        let mut review = Review::new("author");
        review.checks = passing();
        review.apply(Action::RecordChecks).expect("checks");
        review
            .apply(Action::Advance {
                actor: "permission-operator".to_owned(),
            })
            .expect("permission review");
        review
            .apply(Action::Advance {
                actor: "security-operator".to_owned(),
            })
            .expect("security review");
        review
            .apply(Action::Advance {
                actor: "third-operator".to_owned(),
            })
            .expect("to quorum");
        review
    }

    #[test]
    fn the_stages_run_in_order_and_cannot_be_skipped() {
        let mut review = Review::new("author");
        review.checks = passing();
        let refusal = review
            .apply(Action::Advance {
                actor: "operator".to_owned(),
            })
            .expect_err("automated checks have not run");
        assert_eq!(
            refusal,
            Refusal::StageSkipped {
                from: Stage::Submitted,
                to: Stage::AutomatedChecks
            }
        );
        assert_eq!(review.stage, Stage::Submitted);
    }

    #[test]
    fn a_failed_check_stops_the_workflow() {
        let mut review = Review::new("author");
        review.checks = vec![Check {
            name: "sandbox.runs".to_owned(),
            outcome: Err("the module trapped".to_owned()),
        }];
        assert_eq!(
            review.apply(Action::RecordChecks),
            Err(Refusal::ChecksFailed)
        );
        assert_eq!(review.stage, Stage::Submitted);
    }

    #[test]
    fn the_submitter_cannot_review_their_own_package() {
        let mut review = Review::new("author");
        review.checks = passing();
        review.apply(Action::RecordChecks).expect("checks");
        assert_eq!(
            review.apply(Action::Advance {
                actor: "author".to_owned()
            }),
            Err(Refusal::SelfReview)
        );
        // Not as an approver either.
        let mut quorum = at_quorum();
        assert_eq!(
            quorum.apply(Action::Approve {
                actor: "author".to_owned()
            }),
            Err(Refusal::SelfReview)
        );
    }

    #[test]
    fn permission_and_security_review_are_two_people() {
        let mut review = Review::new("author");
        review.checks = passing();
        review.apply(Action::RecordChecks).expect("checks");
        review
            .apply(Action::Advance {
                actor: "operator-a".to_owned(),
            })
            .expect("permission review");
        assert_eq!(
            review.apply(Action::Advance {
                actor: "operator-a".to_owned()
            }),
            Err(Refusal::ReviewersNotIndependent),
            "one person with two headings is one opinion"
        );
    }

    #[test]
    fn quorum_needs_distinct_approvers_and_publishes_on_the_last_one() {
        let mut review = at_quorum();
        assert_eq!(
            review.apply(Action::Approve {
                actor: "operator-a".to_owned()
            }),
            Err(Refusal::QuorumNotReached {
                have: 1,
                need: QUORUM
            })
        );
        assert_eq!(
            review.apply(Action::Approve {
                actor: "operator-a".to_owned()
            }),
            Err(Refusal::DuplicateApproval("operator-a".to_owned()))
        );
        assert_eq!(
            review.apply(Action::Approve {
                actor: "operator-b".to_owned()
            }),
            Ok(Stage::Published)
        );
    }

    #[test]
    fn publishing_is_not_a_stage_somebody_walks_through() {
        let mut review = at_quorum();
        assert!(
            review
                .apply(Action::Advance {
                    actor: "operator".to_owned()
                })
                .is_err(),
            "quorum is reached by approvals, not by advancing"
        );
    }

    #[test]
    fn a_decided_submission_refuses_further_action() {
        let mut review = at_quorum();
        review
            .apply(Action::Approve {
                actor: "operator-a".to_owned(),
            })
            .expect_err("one approval is not quorum");
        review
            .apply(Action::Approve {
                actor: "operator-b".to_owned(),
            })
            .expect("published");
        assert_eq!(
            review.apply(Action::Reject {
                actor: "operator-c".to_owned(),
                reason: "changed my mind".to_owned(),
            }),
            Err(Refusal::AlreadyDecided(Stage::Published)),
            "rejection is for open submissions; a published version is revoked"
        );
    }

    #[test]
    fn revocation_needs_a_published_version() {
        let mut review = at_quorum();
        assert!(review
            .apply(Action::Revoke {
                actor: "operator".to_owned(),
                reason: "emergency".to_owned(),
            })
            .is_err());
        review
            .apply(Action::Approve {
                actor: "operator-a".to_owned(),
            })
            .expect_err("one approval");
        review
            .apply(Action::Approve {
                actor: "operator-b".to_owned(),
            })
            .expect("published");
        assert_eq!(
            review.apply(Action::Revoke {
                actor: "operator".to_owned(),
                reason: "emergency".to_owned(),
            }),
            Ok(Stage::Revoked)
        );
    }

    #[test]
    fn a_wider_permission_set_needs_consent_again_and_a_narrower_one_does_not() {
        let granted: BTreeSet<String> = ["work.read".to_owned()].into();
        let wider: BTreeSet<String> = ["work.read".to_owned(), "network".to_owned()].into();
        let narrower: BTreeSet<String> = BTreeSet::new();
        assert!(Review::needs_renewed_consent(&granted, &wider));
        assert_eq!(
            Review::new_capabilities(&granted, &wider),
            vec!["network"],
            "the prompt names what is new, because a consent that does not say what changed is not consent"
        );
        assert!(!Review::needs_renewed_consent(&granted, &narrower));
        assert!(!Review::needs_renewed_consent(&granted, &granted));
    }
}
