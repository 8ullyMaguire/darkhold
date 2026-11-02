//! M17 — Translation domain: job state machine, unit segmentation, memory, glossaries.

/// Translation job states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranslationJobState {
    Quoted,
    Reserved,
    InProgress,
    InReview,
    Approved,
    Published,
    Failed,
    Cancelled,
}

impl TranslationJobState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Quoted => "quoted",
            Self::Reserved => "reserved",
            Self::InProgress => "in_progress",
            Self::InReview => "in_review",
            Self::Approved => "approved",
            Self::Published => "published",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

impl std::str::FromStr for TranslationJobState {
    type Err = String;
    /// Inverse of [`TranslationJobState::as_str`]; the tests pin the round trip.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "quoted" => Ok(Self::Quoted),
            "reserved" => Ok(Self::Reserved),
            "in_progress" => Ok(Self::InProgress),
            "in_review" => Ok(Self::InReview),
            "approved" => Ok(Self::Approved),
            "published" => Ok(Self::Published),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(format!("unknown translation job state: {s}")),
        }
    }
}

/// Translation unit states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranslationUnitState {
    Pending,
    Translated,
    Reviewed,
    Approved,
}

impl TranslationUnitState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Translated => "translated",
            Self::Reviewed => "reviewed",
            Self::Approved => "approved",
        }
    }
}

impl std::str::FromStr for TranslationUnitState {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(Self::Pending),
            "translated" => Ok(Self::Translated),
            "reviewed" => Ok(Self::Reviewed),
            "approved" => Ok(Self::Approved),
            _ => Err(format!("unknown translation unit state: {s}")),
        }
    }
}

/// Review gate types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewGate {
    Linguistic,
    Cultural,
    Final,
}

impl ReviewGate {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Linguistic => "linguistic",
            Self::Cultural => "cultural",
            Self::Final => "final",
        }
    }
}

impl std::str::FromStr for ReviewGate {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "linguistic" => Ok(Self::Linguistic),
            "cultural" => Ok(Self::Cultural),
            "final" => Ok(Self::Final),
            _ => Err(format!("unknown review gate: {s}")),
        }
    }
}

/// Validate a translation job state transition.
pub fn valid_job_transition(from: &TranslationJobState, to: &TranslationJobState) -> bool {
    matches!(
        (from, to),
        (TranslationJobState::Quoted, TranslationJobState::Reserved)
            | (TranslationJobState::Quoted, TranslationJobState::Cancelled)
            | (
                TranslationJobState::Reserved,
                TranslationJobState::InProgress
            )
            | (TranslationJobState::Reserved, TranslationJobState::Failed)
            | (
                TranslationJobState::InProgress,
                TranslationJobState::InReview
            )
            | (TranslationJobState::InProgress, TranslationJobState::Failed)
            | (TranslationJobState::InReview, TranslationJobState::Approved)
            | (TranslationJobState::InReview, TranslationJobState::Failed)
            | (
                TranslationJobState::Approved,
                TranslationJobState::Published
            )
            | (TranslationJobState::Approved, TranslationJobState::Failed)
    )
}

/// Compute a normalized hash for a paragraph (for memory lookup).
pub fn paragraph_hash(text: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let normalized = text.trim().to_lowercase();
    let mut hasher = DefaultHasher::new();
    normalized.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

/// Apply glossary to text (exact match first, then case-insensitive, then longest match).
pub fn apply_glossary(text: &str, glossary: &[(String, String)], case_sensitive: bool) -> String {
    let mut result = text.to_string();

    // Sort by term length descending (longest match first)
    let mut sorted_glossary: Vec<_> = glossary.iter().collect();
    sorted_glossary.sort_by_key(|(term, _)| std::cmp::Reverse(term.len()));

    for (term, translation) in sorted_glossary {
        if case_sensitive && result.contains(term.as_str()) {
            result = result.replace(term.as_str(), translation.as_str());
        } else if let Some(replaced) = replace_case_insensitive(&result, term, translation) {
            // Case-insensitive fallback: a glossary term must still match at
            // the start of a sentence, where the source capitalises it.
            // (Byte-length indexing assumes ASCII terms; glossaries are.)
            result = replaced;
        }
    }
    result
}

/// Replace every (case-insensitive) occurrence of `term`, splicing into the
/// original text so surrounding capitalisation is preserved. Returns `None`
/// when the term does not occur at all.
fn replace_case_insensitive(hay: &str, term: &str, repl: &str) -> Option<String> {
    let lower_hay = hay.to_lowercase();
    let lower_term = term.to_lowercase();
    let mut out = String::with_capacity(hay.len());
    let mut consumed = 0;
    while let Some(rel) = lower_hay[consumed..].find(&lower_term) {
        let start = consumed + rel;
        let end = start + term.len();
        out.push_str(&hay[consumed..start]);
        out.push_str(repl);
        consumed = end;
    }
    if consumed == 0 {
        return None;
    }
    out.push_str(&hay[consumed..]);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn job_state_round_trip() {
        for s in [
            TranslationJobState::Quoted,
            TranslationJobState::Reserved,
            TranslationJobState::InProgress,
            TranslationJobState::InReview,
            TranslationJobState::Approved,
            TranslationJobState::Published,
            TranslationJobState::Failed,
            TranslationJobState::Cancelled,
        ] {
            assert_eq!(TranslationJobState::from_str(s.as_str()).unwrap(), s);
        }
    }

    #[test]
    fn happy_path_job_transitions() {
        use TranslationJobState::*;
        assert!(valid_job_transition(&Quoted, &Reserved));
        assert!(valid_job_transition(&Reserved, &InProgress));
        assert!(valid_job_transition(&InProgress, &InReview));
        assert!(valid_job_transition(&InReview, &Approved));
        assert!(valid_job_transition(&Approved, &Published));
    }

    #[test]
    fn failure_transitions() {
        use TranslationJobState::*;
        assert!(valid_job_transition(&Reserved, &Failed));
        assert!(valid_job_transition(&InProgress, &Failed));
        assert!(valid_job_transition(&Approved, &Failed));
    }

    #[test]
    fn invalid_transitions() {
        use TranslationJobState::*;
        assert!(!valid_job_transition(&Published, &Quoted));
        assert!(!valid_job_transition(&Failed, &InProgress));
    }

    #[test]
    fn paragraph_hash_deterministic() {
        let h1 = paragraph_hash("Hello World");
        let h2 = paragraph_hash("Hello World");
        assert_eq!(h1, h2);
    }

    #[test]
    fn paragraph_hash_case_insensitive() {
        let h1 = paragraph_hash("Hello World");
        let h2 = paragraph_hash("hello world");
        assert_eq!(h1, h2);
    }

    #[test]
    fn apply_glossary_exact_match() {
        let glossary = vec![
            ("dragon".to_string(), "drago".to_string()),
            ("knight".to_string(), "chevalero".to_string()),
        ];
        let text = "The dragon fought the knight";
        let result = apply_glossary(text, &glossary, true);
        assert_eq!(result, "The drago fought the chevalero");
    }

    #[test]
    fn apply_glossary_longest_match_first() {
        let glossary = vec![
            ("the dragon".to_string(), "la drago".to_string()),
            ("dragon".to_string(), "draco".to_string()),
        ];
        let text = "The dragon appeared";
        let result = apply_glossary(text, &glossary, true);
        // The two-word term matches case-insensitively ("The dragon") and the
        // whole span is consumed by its translation.
        assert_eq!(result, "la drago appeared");
    }

    #[test]
    fn review_gate_round_trip() {
        for g in [
            ReviewGate::Linguistic,
            ReviewGate::Cultural,
            ReviewGate::Final,
        ] {
            assert_eq!(ReviewGate::from_str(g.as_str()).unwrap(), g);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// §61 — cross-language supply (M45-30).
//
// C6 asks for three things: taste-score foreign works, fund translation bounties, with author
// permission. The first is **refused by name** below, because this tree has no stylometry and its
// tag vocabulary is not backfilled (M63-32) — a cross-language taste score would be a number with no
// signal behind it. What is left is measured rather than invented: how much of the catalogue is in
// each language, and whether an author has allowed a translation of a particular work.
//
// The permission half is not new machinery: §40.2 already stores a `PermissionStatement` on every
// work with a `translation` arm, and `GET`/`PUT /works/{id}/permissions` already exist. What was
// missing is §40.2's own enforcement sentence — "the translation pipeline (M17) checks `translation`
// before a request is created" — which is why [`funding_gate`] exists and why `create_translation`
// now consults it.
// ─────────────────────────────────────────────────────────────────────────────

use crate::permission::Permission;

/// A language, as the archive counts it: the **primary subtag**, lowercased.
///
/// `pt-BR`, `pt-br` and `pt` are one language for every purpose this section has, because a supply
/// report that splits Brazilian from European Portuguese before it holds either is a report about
/// tags rather than about readers.
#[must_use]
pub fn normalise_language(tag: &str) -> Option<String> {
    let primary = tag.trim().split(['-', '_']).next()?.trim().to_ascii_lowercase();
    if primary.len() < 2
        || primary.len() > 3
        || !primary.chars().all(|c| c.is_ascii_alphabetic())
    {
        return None;
    }
    Some(primary)
}

/// What the author's `translation` statement permits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Funding {
    /// `translation: yes` — a bounty may be funded and a job requested.
    Fundable,
    /// `translation: ask` — the author must answer first; a bounty would be pressure, not permission.
    MustAsk,
    /// `translation: no` or `unstated` — refused, with the statement named for the refusal.
    Forbidden { statement: &'static str },
}

/// §40.2's model, applied to money: `yes` funds, `ask` asks first, `no` and `unstated` refuse.
///
/// `unstated` refuses because C6 says "author permission required", and §40's own default is the
/// restrictive reading: an archive that funded translations of works whose authors said nothing
/// would be spending an author's silence as consent.
#[must_use]
pub fn funding_gate(permission: Permission) -> Funding {
    match permission {
        Permission::Yes => Funding::Fundable,
        Permission::Ask => Funding::MustAsk,
        Permission::No => Funding::Forbidden { statement: "no" },
        Permission::Unstated => Funding::Forbidden {
            statement: "unstated",
        },
    }
}

/// How much of the catalogue is in each language.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Supply {
    /// Primary subtag -> works. A `BTreeMap` so the report is ordered without a second sort.
    pub by_language: std::collections::BTreeMap<String, i64>,
    /// Works counted, including those with no usable language tag.
    pub total: i64,
}

impl Supply {
    pub fn add(&mut self, language: Option<&str>, count: i64) {
        let key = language
            .and_then(normalise_language)
            .unwrap_or_else(|| "unknown".to_string());
        *self.by_language.entry(key).or_insert(0) += count;
        self.total += count;
    }
}

/// Languages the archive has nothing in, out of the ones asked for.
#[must_use]
pub fn unrepresented(supply: &Supply, wanted: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = wanted
        .iter()
        .filter_map(|tag| normalise_language(tag))
        .filter(|tag| !supply.by_language.contains_key(tag))
        .collect();
    out.dedup();
    out
}

/// Whether cross-language taste transfer can be computed on this instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transfer {
    Available,
    /// Never returned without a reason: `missing` names what the instance would need first.
    Unavailable { missing: Vec<&'static str> },
}

/// The two things this tree lacks, measured 2026-11-02 rather than assumed.
pub const TRANSFER_MISSING: [&str; 2] = ["stylometry", "tag backfill (M63-32)"];

/// C6's "taste-score foreign works", answered as a fact about the build.
#[must_use]
pub fn transfer_available(stylometry: bool, tags_backfilled: bool) -> Transfer {
    let mut missing = Vec::new();
    if !stylometry {
        missing.push(TRANSFER_MISSING[0]);
    }
    if !tags_backfilled {
        missing.push(TRANSFER_MISSING[1]);
    }
    if missing.is_empty() {
        Transfer::Available
    } else {
        Transfer::Unavailable { missing }
    }
}

#[cfg(test)]
mod supply_tests {
    use super::*;

    #[test]
    fn a_language_is_its_primary_subtag() {
        assert_eq!(normalise_language("pt-BR").as_deref(), Some("pt"));
        assert_eq!(normalise_language("PT_br").as_deref(), Some("pt"));
        assert_eq!(normalise_language("  es  ").as_deref(), Some("es"));
        assert_eq!(normalise_language("en").as_deref(), Some("en"));
        assert_eq!(normalise_language(""), None);
        assert_eq!(normalise_language("   "), None);
        assert_eq!(normalise_language("not a language"), None);
        assert_eq!(normalise_language("12"), None);
    }

    #[test]
    fn only_an_explicit_yes_funds_a_translation() {
        assert_eq!(funding_gate(Permission::Yes), Funding::Fundable);
        assert_eq!(funding_gate(Permission::Ask), Funding::MustAsk);
        assert_eq!(
            funding_gate(Permission::No),
            Funding::Forbidden { statement: "no" }
        );
        assert_eq!(
            funding_gate(Permission::Unstated),
            Funding::Forbidden {
                statement: "unstated"
            },
            "silence is not consent: unstated refuses funding"
        );
    }
}

#[cfg(test)]
mod supply_report_tests {
    use super::*;

    #[test]
    fn supply_counts_by_primary_subtag_and_reports_the_total() {
        let mut supply = Supply::default();
        supply.add(Some("en"), 3);
        supply.add(Some("pt-BR"), 2);
        supply.add(Some("pt"), 1);
        supply.add(None, 4);
        assert_eq!(
            supply.by_language.get("pt"),
            Some(&3),
            "pt-BR and pt are one bucket"
        );
        assert_eq!(supply.by_language.get("en"), Some(&3));
        assert_eq!(supply.by_language.get("unknown"), Some(&4));
        assert_eq!(supply.total, 10, "every work is counted once");
    }

    #[test]
    fn unrepresented_names_what_the_archive_has_nothing_in() {
        let mut supply = Supply::default();
        supply.add(Some("en"), 10);
        assert_eq!(
            unrepresented(&supply, &["en", "pt-BR", "es"]),
            vec!["pt".to_string(), "es".to_string()]
        );
        assert!(
            unrepresented(&supply, &["", "??"]).is_empty(),
            "a tag that is not a language is not a request for one"
        );
    }

    #[test]
    fn the_transfer_half_names_what_is_missing_rather_than_answering_zero() {
        assert_eq!(
            transfer_available(false, false),
            Transfer::Unavailable {
                missing: vec!["stylometry", "tag backfill (M63-32)"]
            }
        );
        assert_eq!(
            transfer_available(false, true),
            Transfer::Unavailable {
                missing: vec!["stylometry"]
            }
        );
        assert_eq!(transfer_available(true, true), Transfer::Available);
    }
}
