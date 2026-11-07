//! M16-12's recipe half — a recipe is a closed composition of strategies, and the rule that decides whether
//! running a package hands a reader's data to something else.
//!
//! # Why the schema is closed
//!
//! §16's table says a `recipe` is "a JSON composition of existing parts", runs no code, and carries negligible
//! risk. That sentence is only true if the document cannot carry anything *else*: a recipe with a free-form
//! field is a recipe that can hold a URL, and a recipe with a URL is an extension that phones home while
//! calling itself declarative. So the document has exactly one shape — `stages`, each naming a strategy the
//! instance actually has and a weight — and any other key is refused **by name**, the same way an unknown
//! manifest field is refused: the reviewed text and the running thing must not be two different things.
//!
//! # Why that decides the opt-out
//!
//! §21's last clause is "custom engines and recipes honour opt-out". For a recipe the clause is satisfied by
//! construction rather than by a check that could be forgotten: a document that can name nothing but strategy
//! names cannot send a reader's text anywhere. For an engine the clause bites, because an engine *is* the
//! thing that processes — so [`requires_custom_processing`] answers per category, and the dispatch route asks
//! before it runs anything.

use serde_json::Value;

use crate::extension::Category;

/// The most stages a recipe may have.
///
/// A blend of more than a handful of strategies is not a recipe, it is a search for a number — and every stage
/// is a query the instance pays for on every feed request.
pub const MAX_STAGES: usize = 8;

/// Why a recipe document was refused.
#[derive(Debug, Clone, PartialEq)]
pub enum Refusal {
    /// The document is not an object.
    NotAnObject,
    /// A key the schema does not define. Refused by name so the author knows what to remove.
    UnknownField {
        /// The offending key.
        field: String,
    },
    /// No stages: a recipe that composes nothing is not a recipe.
    NoStages,
    /// More than [`MAX_STAGES`].
    TooManyStages {
        /// How many were supplied.
        stages: usize,
    },
    /// A stage naming a strategy this instance does not run.
    UnknownStrategy {
        /// What the recipe asked for.
        strategy: String,
        /// What the instance has, so the refusal is a menu rather than a complaint.
        known: Vec<String>,
    },
    /// A weight outside `(0, 1]`.
    BadWeight {
        /// The stage's index, from zero.
        stage: usize,
        /// The weight supplied.
        weight: f64,
    },
    /// The same strategy twice, which silently doubles its weight.
    DuplicateStrategy {
        /// The name repeated.
        strategy: String,
    },
    /// A stage that is not an object with `strategy` and `weight`.
    MalformedStage {
        /// The stage's index, from zero.
        stage: usize,
    },
}

impl Refusal {
    /// A sentence an author can act on.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::NotAnObject => "a recipe is an object with one key, `stages`".to_owned(),
            Self::UnknownField { field } => format!(
                "`{field}` is not part of a recipe; the only key is `stages`, and a document that can carry more can carry a URL"
            ),
            Self::NoStages => "a recipe needs at least one stage".to_owned(),
            Self::TooManyStages { stages } => {
                format!("{stages} stages; a recipe may compose at most {MAX_STAGES}")
            }
            Self::UnknownStrategy { strategy, known } => format!(
                "`{strategy}` is not a strategy this instance runs; it has: {}",
                known.join(", ")
            ),
            Self::BadWeight { stage, weight } => format!(
                "stage {stage} has weight {weight}; a weight is greater than 0 and at most 1"
            ),
            Self::DuplicateStrategy { strategy } => format!(
                "`{strategy}` appears twice, which silently doubles its weight"
            ),
            Self::MalformedStage { stage } => {
                format!("stage {stage} must be an object with `strategy` and `weight`")
            }
        }
    }
}

/// One composition step.
#[derive(Debug, Clone, PartialEq)]
pub struct Stage {
    /// A strategy name the instance runs.
    pub strategy: String,
    /// Its weight, in `(0, 1]`.
    pub weight: f64,
}

/// A validated recipe.
#[derive(Debug, Clone, PartialEq)]
pub struct Recipe {
    /// The stages, in order.
    pub stages: Vec<Stage>,
}

impl Recipe {
    /// The document to store, in the same closed shape.
    ///
    /// Built from the parsed value rather than echoed from the request, so what is stored is what was
    /// validated — a recipe that carried something the validator did not look at cannot reach the table.
    #[must_use]
    pub fn document(&self) -> Value {
        serde_json::json!({
            "stages": self
                .stages
                .iter()
                .map(|stage| serde_json::json!({ "strategy": stage.strategy, "weight": stage.weight }))
                .collect::<Vec<Value>>(),
        })
    }

    /// The weights sum, for a caller that wants to report it. Not required to be 1: a recipe is a blend of
    /// rankings with weights, and demanding normalisation would make "add one more strategy" a renumbering
    /// exercise.
    #[must_use]
    pub fn total_weight(&self) -> f64 {
        self.stages.iter().map(|stage| stage.weight).sum()
    }
}

/// Parse and validate a recipe document against the strategies this instance runs.
///
/// `known` is passed in rather than read from a constant: the strategy set is the engine registry's, and two
/// lists of names in two crates drift the moment a strategy is added.
pub fn parse(document: &Value, known: &[String]) -> Result<Recipe, Vec<Refusal>> {
    let Some(object) = document.as_object() else {
        return Err(vec![Refusal::NotAnObject]);
    };

    let mut refusals = Vec::new();
    for key in object.keys() {
        if key != "stages" {
            refusals.push(Refusal::UnknownField { field: key.clone() });
        }
    }

    let stages = match object.get("stages").and_then(Value::as_array) {
        Some(stages) => stages,
        None => {
            refusals.push(Refusal::NoStages);
            return Err(refusals);
        }
    };

    if stages.is_empty() {
        refusals.push(Refusal::NoStages);
    }
    if stages.len() > MAX_STAGES {
        refusals.push(Refusal::TooManyStages {
            stages: stages.len(),
        });
    }

    let mut parsed = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for (index, stage) in stages.iter().enumerate() {
        let (Some(strategy), Some(weight)) = (
            stage.get("strategy").and_then(Value::as_str),
            stage.get("weight").and_then(Value::as_f64),
        ) else {
            refusals.push(Refusal::MalformedStage { stage: index });
            continue;
        };

        // A stage may carry exactly two keys; anything else is where a URL would hide.
        if let Some(object) = stage.as_object() {
            for key in object.keys() {
                if key != "strategy" && key != "weight" {
                    refusals.push(Refusal::UnknownField {
                        field: format!("stages[{index}].{key}"),
                    });
                }
            }
        }

        if !known.iter().any(|k| k == strategy) {
            refusals.push(Refusal::UnknownStrategy {
                strategy: strategy.to_owned(),
                known: known.to_vec(),
            });
            continue;
        }
        if !(weight > 0.0 && weight <= 1.0) {
            refusals.push(Refusal::BadWeight {
                stage: index,
                weight,
            });
            continue;
        }
        if seen.iter().any(|s| s == strategy) {
            refusals.push(Refusal::DuplicateStrategy {
                strategy: strategy.to_owned(),
            });
            continue;
        }

        seen.push(strategy.to_owned());
        parsed.push(Stage {
            strategy: strategy.to_owned(),
            weight,
        });
    }

    if refusals.is_empty() {
        Ok(Recipe { stages: parsed })
    } else {
        Err(refusals)
    }
}

/// What running a category of extension can do to a reader's data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Processing {
    /// Composes parts that are already in the process. Nothing to hand anywhere.
    Composes,
    /// Runs code, or drives an engine outside the process. Data may leave.
    Runs,
}

impl Processing {
    /// Whether the reader's `custom_processing` setting governs running it.
    #[must_use]
    pub fn needs_consent(self) -> bool {
        matches!(self, Self::Runs)
    }
}

/// Whether running this category can hand a reader's data to something outside the process.
///
/// Exactly one category composes instead of running — the declarative recipe — because §16 defines it that
/// way and because [`parse`] enforces it: the document has no field that could carry an address. Every other
/// category runs a module or an engine, so consent applies to it. The narrowness is the point: a broad "these
/// ones are harmless" list is how a category with a network field ends up in it.
#[must_use]
pub fn processing(category: Category) -> Processing {
    match category {
        Category::DeclarativeRecipe => Processing::Composes,
        Category::ReaderWidget
        | Category::DashboardWidget
        | Category::ThemeLayout
        | Category::RecommendationEngine
        | Category::SearchHelper
        | Category::WritingTool
        | Category::ChallengeVariant
        | Category::PositivityFilterRule
        | Category::MoodTag
        | Category::FandomLanding
        | Category::Integration
        | Category::SourceAdapters => Processing::Runs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn known() -> Vec<String> {
        vec![
            "cooccurrence".to_owned(),
            "taste".to_owned(),
            "freshness".to_owned(),
        ]
    }

    #[test]
    fn a_recipe_of_known_strategies_parses_and_stores_only_what_it_parsed() {
        let document = json!({
            "stages": [
                { "strategy": "taste", "weight": 0.6 },
                { "strategy": "cooccurrence", "weight": 0.4 },
            ]
        });
        let recipe = parse(&document, &known()).expect("valid recipe");
        assert_eq!(recipe.stages.len(), 2);
        assert!((recipe.total_weight() - 1.0).abs() < f64::EPSILON);
        assert_eq!(
            recipe.document(),
            document,
            "the stored form is the validated form"
        );
    }

    #[test]
    fn an_unknown_field_is_refused_by_name_because_that_is_where_a_url_would_hide() {
        let document = json!({
            "stages": [{ "strategy": "taste", "weight": 1.0 }],
            "endpoint": "https://tracker.example/collect",
        });
        let refusals = parse(&document, &known()).unwrap_err();
        assert_eq!(refusals.len(), 1);
        assert!(matches!(&refusals[0], Refusal::UnknownField { field } if field == "endpoint"));
        assert!(refusals[0].message().contains("endpoint"));
    }

    #[test]
    fn a_stage_may_not_carry_a_third_key() {
        let document = json!({
            "stages": [
                { "strategy": "taste", "weight": 1.0, "url": "https://tracker.example" }
            ]
        });
        let refusals = parse(&document, &known()).unwrap_err();
        assert!(
            refusals
                .iter()
                .any(|r| matches!(r, Refusal::UnknownField { field } if field.contains("url"))),
            "{refusals:?}"
        );
    }

    #[test]
    fn a_strategy_the_instance_does_not_run_is_refused_with_the_menu() {
        let document = json!({ "stages": [{ "strategy": "oracle", "weight": 1.0 }] });
        let refusals = parse(&document, &known()).unwrap_err();
        match &refusals[0] {
            Refusal::UnknownStrategy { strategy, known } => {
                assert_eq!(strategy, "oracle");
                assert!(known.contains(&"taste".to_owned()));
            }
            other => panic!("{other:?}"),
        }
        assert!(
            refusals[0].message().contains("cooccurrence"),
            "{:?}",
            refusals[0].message()
        );
    }

    #[test]
    fn weights_are_bounded_and_zero_is_not_a_weight() {
        let zero = json!({ "stages": [{ "strategy": "taste", "weight": 0.0 }] });
        assert!(matches!(
            parse(&zero, &known()).unwrap_err()[0],
            Refusal::BadWeight { stage: 0, .. }
        ));

        let over = json!({ "stages": [{ "strategy": "taste", "weight": 1.5 }] });
        assert!(matches!(
            parse(&over, &known()).unwrap_err()[0],
            Refusal::BadWeight { .. }
        ));

        let one = json!({ "stages": [{ "strategy": "taste", "weight": 1.0 }] });
        assert!(parse(&one, &known()).is_ok());
    }

    #[test]
    fn the_same_strategy_twice_is_refused_rather_than_doubled() {
        let document = json!({
            "stages": [
                { "strategy": "taste", "weight": 0.5 },
                { "strategy": "taste", "weight": 0.5 },
            ]
        });
        let refusals = parse(&document, &known()).unwrap_err();
        assert!(matches!(
            &refusals[0],
            Refusal::DuplicateStrategy { strategy } if strategy == "taste"
        ));
    }

    #[test]
    fn an_empty_or_oversized_recipe_is_refused() {
        assert!(matches!(
            parse(&json!({ "stages": [] }), &known()).unwrap_err()[0],
            Refusal::NoStages
        ));

        let many: Vec<Value> = (0..(MAX_STAGES + 1))
            .map(|_| json!({ "strategy": "taste", "weight": 0.1 }))
            .collect();
        let refusals = parse(&json!({ "stages": many }), &known()).unwrap_err();
        assert!(refusals
            .iter()
            .any(|r| matches!(r, Refusal::TooManyStages { .. })));
    }

    #[test]
    fn a_document_that_is_not_an_object_is_refused_rather_than_defaulted() {
        assert_eq!(
            parse(&json!("taste"), &known()).unwrap_err(),
            vec![Refusal::NotAnObject]
        );
        assert_eq!(
            parse(&json!({}), &known()).unwrap_err(),
            vec![Refusal::NoStages]
        );
    }

    #[test]
    fn only_the_declarative_recipe_composes_and_everything_else_runs() {
        assert_eq!(
            processing(Category::DeclarativeRecipe),
            Processing::Composes
        );
        assert!(!processing(Category::DeclarativeRecipe).needs_consent());
        // The engine is the case §21's clause is about: it processes, so consent governs it.
        assert_eq!(processing(Category::RecommendationEngine), Processing::Runs);
        assert!(processing(Category::RecommendationEngine).needs_consent());
        for category in [
            Category::ReaderWidget,
            Category::DashboardWidget,
            Category::ThemeLayout,
            Category::SearchHelper,
            Category::WritingTool,
            Category::ChallengeVariant,
            Category::PositivityFilterRule,
            Category::MoodTag,
            Category::FandomLanding,
            Category::Integration,
            Category::SourceAdapters,
        ] {
            assert!(processing(category).needs_consent(), "{category:?}");
        }
    }
}
