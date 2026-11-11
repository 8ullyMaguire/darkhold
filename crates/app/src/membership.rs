//! The membership gate at the doors (M69-15, second half).
//!
//! The rule lives in `darkhold_domain::membership` and knows nothing about requests. This module is the seam: it reads the
//! three facts the rule needs, applies the configured rules, and turns a standing into an HTTP answer.
//!
//! **Two decisions are recorded here rather than in the rule.**
//!
//! * **The gate is one door wide for now, and it is publishing.** `POST /works` is guarded; comments and forum posts are
//!   not, and that is named as the remaining half in the ledger rather than implied. A probation that silently blocked
//!   everything — including rating, which is how a reader *earns* the floor — would be a gate nobody can graduate from.
//! * **The refusal is `422`, not `403`,** because the estate's envelope carries the detail that makes the refusal
//!   actionable (`field_errors.membership`: the days left and the samples needed) and `AppError` has no variant that pairs a
//!   `403` with field detail. A reader told "access denied" learns nothing; a reader told "nine more ratings or eleven more
//!   days" knows exactly what the instance is asking for. If a `403` variant with detail is added later, this is where it
//!   is used.

use crate::http::{ApiError, ApiResult};
use crate::state::AppState;
use darkhold_domain::membership::{self, Standing};
use darkhold_domain::error::AppError;
use serde_json::{json, Value};

/// What the gate decided about one account, with the numbers that produced it.
#[derive(Debug, Clone, Copy)]
pub struct Membership {
    /// The standing the rule returned.
    pub standing: Standing,
    /// The taste samples the floor asks for at this population.
    pub floor: i64,
    /// The reader count the floor was computed against.
    pub population: i64,
}

impl Membership {
    /// The reader-facing rendering: their standing, and what would change it.
    #[must_use]
    pub fn view(&self) -> Value {
        let mut body = json!({
            "floor": self.floor,
            "population": self.population,
        });
        match self.standing {
            Standing::Full { because } => {
                body["standing"] = json!("full");
                body["because"] = json!(match because {
                    membership::Reason::GateOff => "gate_off",
                    membership::Reason::BelowThreshold => "below_threshold",
                    membership::Reason::Grandfathered => "grandfathered",
                    membership::Reason::Graduated => "graduated",
                });
            }
            Standing::Probation {
                days_left,
                samples_needed,
            } => {
                body["standing"] = json!("probation");
                body["days_left"] = json!(days_left);
                body["samples_needed"] = json!(samples_needed);
                body["finishes_with"] = json!(
                    "rate works you have read: the probation ends when the floor is met, and it ends on its own \
                     when the window closes"
                );
            }
            Standing::Redirect { samples_short } => {
                body["standing"] = json!("redirect");
                body["samples_short"] = json!(samples_short);
                body["passport"] = json!("/api/v1/me/passport");
                body["note"] = json!(
                    "your probation has ended without reaching the floor; your account is intact and your passport \
                     exports everything in it"
                );
            }
        }
        body
    }

    /// The refusal a write door answers with, or `None` when the caller may act.
    #[must_use]
    pub fn refusal(&self) -> Option<ApiError> {
        let (message, detail) = match self.standing {
            Standing::Full { .. } => return None,
            Standing::Probation {
                days_left,
                samples_needed,
            } => (
                "This account is on probation until it has rated enough works.",
                format!(
                    "rate {samples_needed} more work(s) to finish probation; the window closes in {days_left} day(s)"
                ),
            ),
            Standing::Redirect { samples_short } => (
                "This account's probation ended before it reached the floor.",
                format!(
                    "{samples_short} more rating(s) are needed; your passport at /api/v1/me/passport exports \
                     everything in the account"
                ),
            ),
        };
        let mut fields = std::collections::BTreeMap::new();
        fields.insert("membership".to_owned(), detail);
        Some(ApiError(AppError::Validation {
            message: message.to_owned(),
            field_errors: fields,
        }))
    }
}

/// Evaluate the gate for one account.
///
/// The rules are read from configuration on every call rather than cached: the gate is off by default, so the common path
/// is one comparison and no query at all, and an operator who switches it on expects the next request to obey rather than
/// the next restart.
pub async fn evaluate(state: &AppState, account: &str) -> ApiResult<Membership> {
    let governance = state.config().governance.clone();
    let rules = governance.membership_rules();
    if !rules.enabled {
        // No queries at all: with the gate off the answer cannot depend on the database, and a per-request count of ratings
        // on an instance that does not use the feature is a cost with no reader.
        return Ok(Membership {
            standing: Standing::Full {
                because: membership::Reason::GateOff,
            },
            floor: rules.floor_base,
            population: governance.membership_population_threshold,
        });
    }
    let population = darkhold_db::governance::reader_population(state.db())
        .await
        .map_err(|error| ApiError(AppError::internal("reading the reader population", error)))?;
    let facts = darkhold_db::governance::membership_facts(
        state.db(),
        account,
        governance.membership_grandfather_before.as_deref(),
    )
    .await
    .map_err(|error| ApiError(AppError::internal("reading the membership facts", error)))?;
    Ok(Membership {
        standing: membership::decide(&rules, facts, population),
        floor: rules.floor_at(population),
        population,
    })
}
