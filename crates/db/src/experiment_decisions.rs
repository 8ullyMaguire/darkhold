//! Writing the decision down (M64-04).
//!
//! `darkhold_domain::experiment_promotion::decide` is pure: it takes counts and returns a verdict with its numbers. This
//! module is the half that survives — it reads the snapshots, builds the counts, asks for the verdict, and records it when
//! the verdict is one an operator has to live with.
//!
//! # Only verdicts are written
//!
//! A nightly `undecided` row would bury the two rows that matter under a month of "not yet". `evaluate_surface` therefore
//! returns every decision and stores only `promote` and `demote`.
//!
//! # The numbers travel with the row
//!
//! `algorithms_before` and `algorithms_after` are the material fact a rollback undoes, so they are written as text on the
//! row rather than inferred later from a config file that may have changed. The lift and evidence are the same argument: a
//! promotion must be arguable from its own row, not from a query that has moved since.

use anyhow::Result;
use serde::{Deserialize, Serialize};

use darkhold_domain::experiment_promotion::{
    decide, ArmCounts, Decision, GuardrailReading, PromotionInput,
};

use crate::sql_owned;
use crate::Backend;
use crate::Database;

/// One row as the database returns it: the columns of `experiment_decisions` that are read back, in order.
type DecisionRow = (String, String, String, i64, i64, f64, f64, String, String);

/// A stored decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredDecision {
    pub surface_id: String,
    pub day: String,
    pub decision: String,
    pub incumbent_arm: i64,
    pub challenger_arm: i64,
    pub lift: f64,
    pub evidence: f64,
    pub algorithms_before: Vec<String>,
    pub algorithms_after: Vec<String>,
}

/// Sum an arm's impressions, conversions (`events`) and the number of days it has been observed, over every snapshot.
#[derive(Debug, Clone, Copy, Default)]
pub struct ArmTotals {
    pub impressions: i64,
    pub conversions: i64,
    pub days: i64,
}

impl ArmTotals {
    fn counts(self) -> ArmCounts {
        ArmCounts {
            impressions: self.impressions,
            conversions: self.conversions,
        }
    }
}

/// Read one arm's totals for a surface across all of its snapshots.
pub async fn arm_totals(db: &Database, surface_id: &str, arm: i64) -> Result<ArmTotals> {
    let sql = match db.backend() {
        Backend::Sqlite => {
            "SELECT COALESCE(SUM(impressions), 0), COALESCE(SUM(events), 0), COUNT(*) \
             FROM experiment_snapshots WHERE surface_id = ?1 AND arm = ?2"
        }
        Backend::Postgres => {
            "SELECT COALESCE(SUM(impressions), 0)::bigint, COALESCE(SUM(events), 0)::bigint, COUNT(*)::bigint \
             FROM experiment_snapshots WHERE surface_id = $1 AND arm = $2"
        }
    };
    let (impressions, conversions, days) = match db.backend() {
        Backend::Sqlite => {
            let row: (i64, i64, i64) = sqlx::query_as(sql)
                .bind(surface_id)
                .bind(arm)
                .fetch_one(db.sqlite_pool().expect("sqlite pool for sqlite backend"))
                .await?;
            row
        }
        Backend::Postgres => {
            let row: (i64, i64, i64) = sqlx::query_as(sql)
                .bind(surface_id)
                .bind(arm)
                .fetch_one(
                    db.postgres_pool()
                        .expect("postgres pool for postgres backend"),
                )
                .await?;
            row
        }
    };
    Ok(ArmTotals {
        impressions,
        conversions,
        days,
    })
}

/// Evaluate two arms of a surface and store the verdict if it is one that changes something.
///
/// Guardrails are read from the caller: no guardrail source has been built yet (M64-15 owns the console and the thresholds),
/// so this takes an empty history and says so rather than pretending a guardrail was checked.
pub async fn evaluate_surface(
    db: &Database,
    surface_id: &str,
    day: &str,
    incumbent_arm: i64,
    challenger_arm: i64,
    swap: (&[String], &[String]),
    guardrails: Vec<Vec<GuardrailReading>>,
) -> Result<Decision> {
    let (algorithms_before, algorithms_after) = swap;
    let incumbent = arm_totals(db, surface_id, incumbent_arm).await?;
    let challenger = arm_totals(db, surface_id, challenger_arm).await?;
    let input = PromotionInput {
        incumbent: incumbent.counts(),
        challenger: challenger.counts(),
        // Days are days *observed*, which is what the snapshot table can answer: an experiment that ran for nine days and
        // produced no traffic has nine rows with zero impressions, not zero rows.
        days: incumbent.days.min(challenger.days),
        guardrails,
    };
    let decision = decide(&input);

    match &decision {
        Decision::Promote { lift, evidence } => {
            record(
                db,
                surface_id,
                day,
                "promote",
                incumbent_arm,
                challenger_arm,
                *lift,
                *evidence,
                None,
                None,
                0,
                algorithms_before,
                algorithms_after,
            )
            .await?;
        }
        Decision::Demote { name, consecutive } => {
            record(
                db,
                surface_id,
                day,
                "demote",
                incumbent_arm,
                challenger_arm,
                0.0,
                0.0,
                Some(name.clone()),
                None,
                *consecutive,
                algorithms_before,
                algorithms_after,
            )
            .await?;
        }
        _ => {}
    }
    Ok(decision)
}

#[allow(clippy::too_many_arguments)]
async fn record(
    db: &Database,
    surface_id: &str,
    day: &str,
    decision: &str,
    incumbent_arm: i64,
    challenger_arm: i64,
    lift: f64,
    evidence: f64,
    guardrail_name: Option<String>,
    guardrail_value: Option<f64>,
    consecutive: usize,
    algorithms_before: &[String],
    algorithms_after: &[String],
) -> Result<()> {
    let sql = sql_owned(
        db,
        "INSERT INTO experiment_decisions \
         (id, surface_id, day, decision, incumbent_arm, challenger_arm, lift, evidence, guardrail_name, \
          guardrail_value, consecutive, algorithms_before, algorithms_after, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, datetime('now'))"
            .into(),
        "INSERT INTO experiment_decisions \
         (id, surface_id, day, decision, incumbent_arm, challenger_arm, lift, evidence, guardrail_name, \
          guardrail_value, consecutive, algorithms_before, algorithms_after, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, now())"
            .into(),
    );
    let id = uuid::Uuid::new_v4().to_string();
    let before = serde_json::to_string(algorithms_before)?;
    let after = serde_json::to_string(algorithms_after)?;
    let consecutive = i64::try_from(consecutive).unwrap_or(i64::MAX);
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(&id)
                .bind(surface_id)
                .bind(day)
                .bind(decision)
                .bind(incumbent_arm)
                .bind(challenger_arm)
                .bind(lift)
                .bind(evidence)
                .bind(guardrail_name)
                .bind(guardrail_value)
                .bind(consecutive)
                .bind(&before)
                .bind(&after)
                .execute(db.sqlite_pool().expect("sqlite pool for sqlite backend"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(&id)
                .bind(surface_id)
                .bind(day)
                .bind(decision)
                .bind(incumbent_arm)
                .bind(challenger_arm)
                .bind(lift)
                .bind(evidence)
                .bind(guardrail_name)
                .bind(guardrail_value)
                .bind(consecutive)
                .bind(&before)
                .bind(&after)
                .execute(
                    db.postgres_pool()
                        .expect("postgres pool for postgres backend"),
                )
                .await?;
        }
    }
    Ok(())
}

/// Every decision recorded for a surface, newest first.
pub async fn decisions_for(db: &Database, surface_id: &str) -> Result<Vec<StoredDecision>> {
    let sql = match db.backend() {
        Backend::Sqlite => {
            "SELECT surface_id, day, decision, incumbent_arm, challenger_arm, lift, evidence, \
                    algorithms_before, algorithms_after \
             FROM experiment_decisions WHERE surface_id = ?1 ORDER BY created_at DESC"
        }
        Backend::Postgres => {
            "SELECT surface_id, day, decision, incumbent_arm, challenger_arm, lift, evidence, \
                    algorithms_before, algorithms_after \
             FROM experiment_decisions WHERE surface_id = $1 ORDER BY created_at DESC"
        }
    };
    let rows: Vec<DecisionRow> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(sql)
                .bind(surface_id)
                .fetch_all(db.sqlite_pool().expect("sqlite pool for sqlite backend"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(sql)
                .bind(surface_id)
                .fetch_all(
                    db.postgres_pool()
                        .expect("postgres pool for postgres backend"),
                )
                .await?
        }
    };
    let mut decisions = Vec::new();
    for (surface_id, day, decision, incumbent_arm, challenger_arm, lift, evidence, before, after) in
        rows
    {
        decisions.push(StoredDecision {
            surface_id,
            day,
            decision,
            incumbent_arm,
            challenger_arm,
            lift,
            evidence,
            algorithms_before: serde_json::from_str(&before).unwrap_or_default(),
            algorithms_after: serde_json::from_str(&after).unwrap_or_default(),
        });
    }
    Ok(decisions)
}
