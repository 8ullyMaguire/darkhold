//! The bridge between darkhold's data and the estate's taste fold (ADR 0016).
//!
//! This file is allowed to know darkhold's nouns; the crate is not. Everything product-specific lives here: which column is
//! a trust level, which flag is `curator`, which local verb is a `save`, and which account the operator is.
//!
//! # Why the operator is a role and not a special case
//!
//! `holds_estate_claim` already treats "the operator" as two spellings of one claim: a `role:operator` token, or the
//! account named in `[administration] operator_account_id`. The fold needs the same claim for rows it reads out of the
//! database, where no token exists, so the configured account is given the same `role:operator` string the token carries.
//! One authority, two sources of the same word.

use darkhold_db::taste_source::{ContributorRow, EvidenceRow};
use polaris_taste_source::{Contributor, Declaration, Evidence, Kind, KindMap, Selector, Source};

use crate::config::{TasteConfig, TasteSourceRule};

/// darkhold's own verbs, mapped onto the estate's vocabulary.
///
/// A verb that is not here does not contribute, and that is deliberate: the store's `evidence` query writes exactly these
/// three words, so a fourth appearing means somebody changed the store and must declare it here.
#[must_use]
pub fn kind_map() -> KindMap {
    KindMap::new([
        ("rating", Kind::Rating),
        ("collection", Kind::CollectionAdd),
        ("saved_search", Kind::SavedSearch),
    ])
}

/// The instance's declared sources, validated.
///
/// Every fault is a sentence naming the source, because this value is produced from a file an operator edited by hand and
/// the answer has to be actionable without a backtrace.
pub fn declaration(taste: &TasteConfig) -> Result<Declaration, String> {
    if taste.taste_source.is_empty() {
        return Err(
            "no [taste.source] entries: this instance has not declared who sets its taste, so there is nothing to fold"
                .to_string(),
        );
    }
    let mut sources = Vec::with_capacity(taste.taste_source.len());
    for rule in &taste.taste_source {
        sources.push(source_from(rule)?);
    }
    let declaration = Declaration {
        sources,
        half_life_days: taste.taste_half_life_days,
        min_evidence: taste.taste_min_evidence,
    };
    declaration
        .validate()
        .map_err(|invalid| format!("[taste] declaration is invalid: {invalid:?}"))?;
    Ok(declaration)
}

fn source_from(rule: &TasteSourceRule) -> Result<Source, String> {
    let selector = match (
        &rule.role,
        &rule.badge,
        rule.trust_at_least,
        rule.affinity_at_least,
        rule.ids.is_empty(),
        rule.everyone,
    ) {
        (Some(role), None, None, None, true, false) => Selector::Role { role: role.clone() },
        (None, Some(badge), None, None, true, false) => Selector::Badge {
            badge: badge.clone(),
        },
        (None, None, Some(at_least), None, true, false) => Selector::TrustLevel { at_least },
        // The evidence floor is the instance's minimum here: a source that needed its own floor would be a second
        // declaration of the same idea, and the fold already refuses a thin corpus.
        (None, None, None, Some(at_least), true, false) => Selector::Affinity {
            at_least,
            min_evidence: rule.ids.len() as u32,
        },
        (None, None, None, None, false, false) => Selector::Explicit {
            ids: rule.ids.clone(),
        },
        (None, None, None, None, true, true) => Selector::Everyone,
        _ => {
            return Err(format!(
                "taste.source {:?} must name exactly one of: role, badge, trust_at_least, affinity_at_least, ids, everyone",
                rule.name
            ));
        }
    };
    let mut kinds = Vec::with_capacity(rule.kinds.len());
    for name in &rule.kinds {
        let kind = Kind::parse(name).ok_or_else(|| {
            format!(
                "taste.source {:?} declares kind {:?}, which is not in the vocabulary ([{}])",
                rule.name,
                name,
                Kind::ALL.map(Kind::name).join(", ")
            )
        })?;
        kinds.push(kind);
    }
    Ok(Source {
        name: rule.name.clone(),
        selector,
        weight: rule.weight,
        kinds,
    })
}

/// The rows, as the fold sees them.
#[must_use]
pub fn contributors(rows: &[ContributorRow], operator_account: Option<&str>) -> Vec<Contributor> {
    rows.iter()
        .map(|row| {
            let mut roles = Vec::new();
            if operator_account == Some(row.id.as_str()) {
                roles.push("role:operator".to_string());
            }
            if row.is_curator {
                roles.push("role:curator".to_string());
            }
            Contributor {
                id: row.id.clone(),
                roles,
                trust_level: row.trust_level,
                badges: row.badges.clone(),
                affinity: row.affinity,
                affinity_evidence: row.affinity_evidence,
            }
        })
        .collect()
}

/// One evidence row, owned, so a `Vec` of them can be borrowed into `Evidence` without a lifetime ladder.
#[derive(Debug, Clone, PartialEq)]
pub struct OwnedEvidence {
    /// The account that acted.
    pub contributor: String,
    /// The act, in the estate's vocabulary.
    pub kind: Kind,
    /// What was acted on.
    pub subject: String,
    /// 0.0-1.0.
    pub strength: f64,
    /// Days since the act.
    pub age_days: f64,
}

impl OwnedEvidence {
    /// Borrow as the crate's row type.
    #[must_use]
    pub fn as_row(&self) -> Evidence<'_> {
        Evidence {
            contributor: &self.contributor,
            kind: self.kind,
            subject: &self.subject,
            strength: self.strength,
            age_days: self.age_days,
        }
    }
}

/// Map the store's rows, and report the verbs it did not recognise rather than dropping them silently.
#[must_use]
pub fn evidence(rows: &[EvidenceRow], map: &KindMap) -> (Vec<OwnedEvidence>, Vec<String>) {
    let mut owned = Vec::with_capacity(rows.len());
    let mut unmapped: Vec<String> = Vec::new();
    for row in rows {
        match map.translate(&row.verb) {
            Some(kind) => owned.push(OwnedEvidence {
                contributor: row.contributor.clone(),
                kind,
                subject: row.subject.clone(),
                strength: row.strength,
                age_days: row.age_days,
            }),
            None => {
                if !unmapped.contains(&row.verb) {
                    unmapped.push(row.verb.clone());
                }
            }
        }
    }
    (owned, unmapped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TasteConfig;

    fn rule(name: &str, kinds: &[&str]) -> TasteSourceRule {
        TasteSourceRule {
            name: name.to_string(),
            role: Some("curator".to_string()),
            badge: None,
            trust_at_least: None,
            affinity_at_least: None,
            ids: Vec::new(),
            everyone: false,
            weight: 0.6,
            kinds: kinds.iter().map(|k| (*k).to_string()).collect(),
        }
    }

    fn config_with(rules: Vec<TasteSourceRule>) -> TasteConfig {
        TasteConfig {
            taste_source: rules,
            ..TasteConfig::default()
        }
    }

    #[test]
    fn a_declared_source_becomes_a_selector_rule() {
        let declaration = declaration(&config_with(vec![rule(
            "curators",
            &["seed", "collection_add"],
        )]))
        .expect("a well-formed declaration");
        assert_eq!(declaration.sources.len(), 1);
        assert_eq!(
            declaration.sources[0].selector,
            Selector::Role {
                role: "curator".to_string()
            }
        );
        assert_eq!(
            declaration.sources[0].kinds,
            vec![Kind::Seed, Kind::CollectionAdd]
        );
    }

    #[test]
    fn no_declared_source_is_a_refusal_not_an_empty_fold() {
        let error =
            declaration(&config_with(vec![])).expect_err("an instance with no sources must say so");
        assert!(error.contains("no [taste.source] entries"), "{error}");
    }

    #[test]
    fn an_unknown_kind_names_the_vocabulary_in_the_error() {
        let error = declaration(&config_with(vec![rule("curators", &["bookmark"])]))
            .expect_err("an undeclared kind is refused");
        assert!(
            error.contains("bookmark") && error.contains("seed"),
            "{error}"
        );
    }

    #[test]
    fn a_source_naming_two_selectors_is_refused() {
        let mut two = rule("both", &["seed"]);
        two.badge = Some("attuned".to_string());
        let error =
            declaration(&config_with(vec![two])).expect_err("two selectors is an ambiguous rule");
        assert!(error.contains("exactly one of"), "{error}");
    }

    #[test]
    fn the_configured_operator_carries_the_same_claim_a_token_would() {
        let rows = vec![
            ContributorRow {
                id: "op".to_string(),
                is_curator: false,
                trust_level: 0,
                affinity: None,
                affinity_evidence: 0,
                badges: Vec::new(),
            },
            ContributorRow {
                id: "cur".to_string(),
                is_curator: true,
                trust_level: 2,
                affinity: Some(0.7),
                affinity_evidence: 9,
                badges: vec!["attuned".to_string()],
            },
        ];
        let mapped = contributors(&rows, Some("op"));
        assert_eq!(mapped[0].roles, vec!["role:operator".to_string()]);
        assert_eq!(mapped[1].roles, vec!["role:curator".to_string()]);
        assert_eq!(mapped[1].badges, vec!["attuned".to_string()]);
    }

    #[test]
    fn an_unmapped_verb_is_reported_once() {
        let rows = vec![
            EvidenceRow {
                contributor: "a".to_string(),
                verb: "favourite".to_string(),
                subject: "tag".to_string(),
                strength: 1.0,
                age_days: 0.0,
            },
            EvidenceRow {
                contributor: "a".to_string(),
                verb: "favourite".to_string(),
                subject: "other".to_string(),
                strength: 1.0,
                age_days: 0.0,
            },
        ];
        let (owned, unmapped) = evidence(&rows, &kind_map());
        assert!(owned.is_empty());
        assert_eq!(unmapped, vec!["favourite".to_string()]);
    }
}
