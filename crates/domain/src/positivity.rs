//! Positivity filter and feedback delivery (spec section 12).
//!
//! Priority 3 shapes this module: only positive or constructive criticism
//! reaches authors, constructive critique requires opt-in, nothing
//! destructive is ever shown. Classification is pure and rule-based:
//! deterministic, offline, inspectable. The spec requires the rules layer to
//! work with no AI provider configured.
//!
//! Pipeline: submitted -> allow/deny check -> classify -> resolve against
//! the author preferences -> delivered | held. Held text is stored but never
//! listed publicly and never shown to the author unasked.
//!
//! Two properties: preferences apply before storage (a change moves
//! subsequent reviews only, stored outcomes are never rewritten); the sender
//! learns nothing about author settings (only posted vs held for review).
//!
//! Everything here is pure: no database, no clock, no I/O.

/// What the classifier decided a text is (spec 12.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FeedbackClass {
    Positive,
    Constructive,
    Ambiguous,
    Negative,
}

impl FeedbackClass {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Positive => "positive",
            Self::Constructive => "constructive",
            Self::Ambiguous => "ambiguous",
            Self::Negative => "negative",
        }
    }
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "positive" => Some(Self::Positive),
            "constructive" => Some(Self::Constructive),
            "ambiguous" => Some(Self::Ambiguous),
            "negative" => Some(Self::Negative),
            _ => None,
        }
    }
}

/// Classifier verdict: class plus confidence in basis points (0..10000, an
/// integer so it binds as i64 on both dialects) plus category signals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    pub class: FeedbackClass,
    pub confidence_bp: i64,
    pub signals: Vec<String>,
}

/// Author feedback preferences (spec 8.4, 12.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedbackPreferences {
    pub accept_constructive: bool,
    pub ambiguous_auto: bool,
    pub comments_enabled: bool,
}

impl Default for FeedbackPreferences {
    fn default() -> Self {
        Self {
            accept_constructive: false,
            ambiguous_auto: false,
            comments_enabled: true,
        }
    }
}

/// Per-work overrides. None means inherit the account default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkFeedbackOverride {
    pub accept_constructive: Option<bool>,
    pub ambiguous_auto: Option<bool>,
    pub comments_enabled: Option<bool>,
}

/// Policy in force for one work: overrides win, account fills the rest.
#[must_use]
pub fn effective(
    account: &FeedbackPreferences,
    work: &WorkFeedbackOverride,
) -> FeedbackPreferences {
    FeedbackPreferences {
        accept_constructive: work
            .accept_constructive
            .unwrap_or(account.accept_constructive),
        ambiguous_auto: work.ambiguous_auto.unwrap_or(account.ambiguous_auto),
        comments_enabled: work.comments_enabled.unwrap_or(account.comments_enabled),
    }
}

/// Where a classified text goes (spec 12.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryOutcome {
    Delivered,
    Held,
}

impl DeliveryOutcome {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Delivered => "delivered",
            Self::Held => "held",
        }
    }
}

/// Decide delivery. Deny holds everything; allow delivers everything; then
/// the class and preference matrix. A paused work holds all non-allowlisted.
#[must_use]
pub fn resolve_delivery(
    class: FeedbackClass,
    prefs: &FeedbackPreferences,
    allow: bool,
    deny: bool,
) -> DeliveryOutcome {
    if deny {
        return DeliveryOutcome::Held;
    }
    if allow {
        return DeliveryOutcome::Delivered;
    }
    if !prefs.comments_enabled {
        return DeliveryOutcome::Held;
    }
    match class {
        FeedbackClass::Positive => DeliveryOutcome::Delivered,
        FeedbackClass::Constructive if prefs.accept_constructive => DeliveryOutcome::Delivered,
        FeedbackClass::Constructive => DeliveryOutcome::Held,
        FeedbackClass::Ambiguous if prefs.ambiguous_auto => DeliveryOutcome::Delivered,
        FeedbackClass::Ambiguous => DeliveryOutcome::Held,
        FeedbackClass::Negative => DeliveryOutcome::Held,
    }
}

/// The only sender-visible string (spec 12.4). Never the class or reason.
#[must_use]
pub const fn sender_receipt(outcome: DeliveryOutcome) -> &'static str {
    match outcome {
        DeliveryOutcome::Delivered => "Comment posted.",
        DeliveryOutcome::Held => "Comment held for moderator review.",
    }
}

/// One-line effective policy for the preferences panel.
#[must_use]
pub fn describe_policy(prefs: &FeedbackPreferences) -> String {
    format!(
        "You receive praise; constructive critique {}; ambiguous feedback {}; comments {}.",
        if prefs.accept_constructive {
            "on"
        } else {
            "off"
        },
        if prefs.ambiguous_auto {
            "delivered"
        } else {
            "held for review"
        },
        if prefs.comments_enabled {
            "open"
        } else {
            "paused"
        }
    )
}

// ---------------------------------------------------------------------------
// Rules classifier
// ---------------------------------------------------------------------------

/// Hostility markers: personal-attack shaped, not merely negative words.
/// "The pacing was garbage" is harsh but about the work; "you are garbage"
/// is about the person. Keep this list short: every entry is a
/// precision/recall bet, and widening it is a policy change.
const HOSTILE_MARKERS: &[&str] = &[
    "you are stupid",
    "you are an idiot",
    "you idiot",
    "shut up",
    "kill yourself",
    "you suck",
    "worthless author",
    "moron",
    "pathetic",
    "loser",
    "hate you",
    "stupid author",
];

/// Craft-specificity markers: the signature of critique, not cruelty.
const CONSTRUCTIVE_MARKERS: &[&str] = &[
    "pacing",
    "typo",
    "grammar",
    "plot hole",
    "continuity",
    "character development",
    "dialogue",
    "tense",
    " pov ",
    "suggestion",
    "consider ",
    "could improve",
    "felt rushed",
    "confusing",
];

/// Dislike markers: verdicts **about the work**, which are neither hostility (that is about the person) nor craft-specificity
/// (that is actionable critique). They exist because the branch below used to have a hole: text with no marker at all fell into the
/// "non-critical engagement" default and was called positive, so `"This is awful and boring. I hate it."` reached the author as praise
/// and inflated `reader_positivity`.
///
/// Every entry is a precision/recall bet, like `HOSTILE_MARKERS`, so the rule is deliberately conservative: **two distinct markers**
/// with nothing craft-specific around them. One harsh adjective is a reader's colour ("the villain was awful" is praise of the
/// antagonist); two without craft-specificity is a verdict nobody can act on.
const DISLIKE_MARKERS: &[&str] = &[
    "awful",
    "boring",
    "hate it",
    "hated it",
    "waste of time",
    "terrible",
    "disappointing",
    "disappointed",
    "couldn't finish",
    "could not finish",
    "not worth",
    "overrated",
    "mediocre",
    "unreadable",
    "poorly written",
];

/// Words that invert the marker after them. A negation is not a detail: "I don't hate it" contains the marker it argues against.
const NEGATIONS: &[&str] = &["not ", "n't ", "no ", "never ", "dont ", "don't ", "didn't ", "isn't ", "wasn't ", "without "];

/// Distinct dislike markers that are not negated by the few words before them.
fn dislikes(haystack: &str) -> Vec<String> {
    DISLIKE_MARKERS
        .iter()
        .filter(|needle| {
            let mut from = 0;
            while let Some(position) = haystack[from..].find(**needle) {
                let at = from + position;
                let before = &haystack[haystack[..at].char_indices().rev().take(12).last().map_or(0, |(i, _)| i)..at];
                if !NEGATIONS.iter().any(|negation| before.contains(negation)) {
                    return true;
                }
                from = at + needle.len();
            }
            false
        })
        .map(|needle| (*needle).to_owned())
        .collect()
}

/// Appreciation markers (pre-classified-positive family, spec 12.7).
const POSITIVE_MARKERS: &[&str] = &[
    "love",
    "great",
    "amazing",
    "wonderful",
    "thank",
    "enjoyed",
    "beautiful",
    "brilliant",
    "favorit",
    "comforting",
    "made me cry",
    "banter",
    "need more of this",
];

fn contains_any(haystack: &str, needles: &[&str]) -> Vec<String> {
    needles
        .iter()
        .filter(|n| haystack.contains(*n))
        .map(|n| (*n).to_owned())
        .collect()
}

fn caps_ratio(text: &str) -> f64 {
    let letters: Vec<char> = text.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.len() < 20 {
        return 0.0;
    }
    let upper = letters.iter().filter(|c| c.is_uppercase()).count();
    upper as f64 / letters.len() as f64
}

/// Classify one text. Deterministic: same input always yields same class.
#[must_use]
pub fn classify(text: &str) -> Classification {
    let folded = text.to_lowercase();
    let trimmed = folded.trim();
    if trimmed.is_empty() {
        return Classification {
            class: FeedbackClass::Ambiguous,
            confidence_bp: 3000,
            signals: vec!["no-signal".to_owned()],
        };
    }
    let hostile = contains_any(trimmed, HOSTILE_MARKERS);
    if !hostile.is_empty() {
        let mut signals = vec!["hostility-pattern".to_owned()];
        signals.extend(hostile.into_iter().take(2));
        return Classification {
            class: FeedbackClass::Negative,
            confidence_bp: 8500,
            signals,
        };
    }
    if caps_ratio(text) > 0.7 {
        let mut signals = vec!["all-caps".to_owned()];
        signals.extend(
            contains_any(trimmed, CONSTRUCTIVE_MARKERS)
                .into_iter()
                .take(1),
        );
        return Classification {
            class: FeedbackClass::Ambiguous,
            confidence_bp: 4500,
            signals,
        };
    }
    let constructive = contains_any(trimmed, CONSTRUCTIVE_MARKERS);
    if !constructive.is_empty() {
        let confidence = 6000 + (constructive.len() as i64 * 1000).min(3000);
        let mut signals = vec!["craft-specificity".to_owned()];
        signals.extend(constructive.into_iter().take(2));
        return Classification {
            class: FeedbackClass::Constructive,
            confidence_bp: confidence,
            signals,
        };
    }
    // A verdict about the work with no way to act on it. Two distinct markers, because one harsh adjective is colour rather than a
    // verdict ("the villain was awful" praises the antagonist), and negation-filtered, because "I don't hate it" is not dislike.
    let dislike = dislikes(trimmed);
    if dislike.len() >= 2 {
        let mut signals = vec!["dislike-without-craft-specificity".to_owned()];
        signals.extend(dislike.into_iter().take(2));
        return Classification {
            class: FeedbackClass::Negative,
            confidence_bp: 6500,
            signals,
        };
    }

    // Anything non-hostile without craft-specificity is non-critical
    // engagement, which the spec counts as positive. The appreciation
    // markers raise confidence; their absence lowers it but does not make
    // the text ambiguous -- otherwise every plain "a quiet story" would
    // route to moderation and pre-filter reviews would vanish behind the
    // gate. Ambiguous is reserved for no signal at all (empty) and for
    // shouting (the all-caps path above).
    let positive = contains_any(trimmed, POSITIVE_MARKERS);
    if !positive.is_empty() {
        let confidence = 6000 + (positive.len() as i64 * 1000).min(3000);
        let mut signals = vec!["appreciation".to_owned()];
        signals.extend(positive.into_iter().take(2));
        return Classification {
            class: FeedbackClass::Positive,
            confidence_bp: confidence,
            signals,
        };
    }
    Classification {
        class: FeedbackClass::Positive,
        confidence_bp: 5500,
        signals: vec!["non-critical-engagement".to_owned()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn praise_is_positive() {
        let v = classify("I loved this unit, the banter was wonderful!");
        assert_eq!(v.class, FeedbackClass::Positive);
        assert!(v.signals.contains(&"appreciation".to_owned()));
    }
    #[test]
    fn craft_feedback_is_constructive() {
        let v = classify("I enjoyed it, but the pacing felt rushed; consider a typo pass.");
        assert_eq!(v.class, FeedbackClass::Constructive);
    }
    #[test]
    fn cruelty_is_negative() {
        let v = classify("You idiot, shut up and stop writing");
        assert_eq!(v.class, FeedbackClass::Negative);
        assert!(v.signals.contains(&"hostility-pattern".to_owned()));
    }
    #[test]
    fn shouting_without_content_is_ambiguous() {
        let v = classify("THIS IS THE WORST THING I HAVE EVER READ ANYWHERE");
        assert_eq!(v.class, FeedbackClass::Ambiguous);
        assert!(v.signals.contains(&"all-caps".to_owned()));
    }
    // The gap pass 81 measured: a verdict with no craft-specific content and no personal attack fell through every branch into the
    // "non-critical engagement" default, so `"This is awful and boring. I hate it."` was **Positive** — delivered to the author as
    // praise, and counted as praise by every consumer of `classify`, including the `reader_positivity` quality signal.
    #[test]
    fn a_generic_verdict_is_not_praise() {
        let v = classify("This is awful and boring. I hate it.");
        assert_eq!(v.class, FeedbackClass::Negative, "{v:?}");
        assert!(
            v.signals.contains(&"dislike-without-craft-specificity".to_owned()),
            "{v:?}"
        );
    }

    // The precision bet in the other direction: one harsh adjective is not a verdict. "The villain was awful" is a reader telling
    // the author the antagonist worked; calling it Negative would hold praise behind the gate.
    #[test]
    fn one_harsh_adjective_is_still_engagement() {
        assert_eq!(classify("The villain was awful.").class, FeedbackClass::Positive);
        assert_eq!(classify("Awful ending, but I loved it.").class, FeedbackClass::Positive);
    }

    // Negation is not a small detail here: "I don't hate it" is the opposite of the marker it contains.
    #[test]
    fn a_negated_dislike_is_not_negative() {
        // Two markers, both negated: this is the pair that proves the negation filter is doing work. With one marker the threshold
        // alone would answer `Positive`, and the test would pass with the filter deleted — measured, the first version of this test
        // survived exactly that mutation.
        assert_eq!(
            classify("I don't hate it and it's not boring at all.").class,
            FeedbackClass::Positive
        );
        assert_eq!(classify("I don't hate it, honestly.").class, FeedbackClass::Positive);
    }

    // Craft-specificity outranks the verdict: disliking a work *and* saying what to fix is critique, which authors can opt into —
    // holding it would punish the reader who did the work of explaining.
    #[test]
    fn actionable_critique_outranks_dislike() {
        // Two dislike markers *and* craft-specificity: the ordering is what the test pins. Swapping the two branches leaves this
        // text `Negative`, and the first version of this test ("Boring in places; …") could not tell — it carried only one dislike
        // marker, so the threshold answered for the ordering and the mutation survived.
        assert_eq!(
            classify("Awful and boring; the pacing drags and the dialogue is thin.").class,
            FeedbackClass::Constructive
        );
    }

    #[test]
    fn classification_is_deterministic() {
        let text = "The dialogue could improve, but thank you for sharing!";
        assert_eq!(classify(text), classify(text));
    }
    #[test]
    fn delivery_matrix() {
        let default = FeedbackPreferences::default();
        let opted = FeedbackPreferences {
            accept_constructive: true,
            ..FeedbackPreferences::default()
        };
        assert_eq!(
            resolve_delivery(FeedbackClass::Positive, &default, false, false),
            DeliveryOutcome::Delivered
        );
        assert_eq!(
            resolve_delivery(FeedbackClass::Constructive, &default, false, false),
            DeliveryOutcome::Held
        );
        assert_eq!(
            resolve_delivery(FeedbackClass::Constructive, &opted, false, false),
            DeliveryOutcome::Delivered
        );
        assert_eq!(
            resolve_delivery(FeedbackClass::Negative, &opted, false, false),
            DeliveryOutcome::Held
        );
        assert_eq!(
            resolve_delivery(FeedbackClass::Positive, &default, false, true),
            DeliveryOutcome::Held
        );
        assert_eq!(
            resolve_delivery(FeedbackClass::Negative, &default, true, false),
            DeliveryOutcome::Delivered
        );
    }
    #[test]
    fn work_overrides_win() {
        let account = FeedbackPreferences::default();
        let work = WorkFeedbackOverride {
            accept_constructive: Some(true),
            ..WorkFeedbackOverride::default()
        };
        assert!(effective(&account, &work).accept_constructive);
        assert!(!effective(&account, &WorkFeedbackOverride::default()).accept_constructive);
    }
    #[test]
    fn receipts_reveal_nothing() {
        assert_eq!(
            sender_receipt(DeliveryOutcome::Delivered),
            "Comment posted."
        );
        assert_eq!(
            sender_receipt(DeliveryOutcome::Held),
            "Comment held for moderator review."
        );
    }
}
