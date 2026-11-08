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
pub(crate) async fn arm_totals(db: &Database, surface_id: &str, arm: i64) -> Result<ArmTotals> {
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
            let decision_id = record(
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
                "automatic",
            )
            .await?;
            // The swap the row describes, made real. A promotion that only writes a row is a note, not a promotion.
            install_override(db, surface_id, algorithms_after, Some(&decision_id)).await?;
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
                "automatic",
            )
            .await?;
            // Demotion restores what was configured before any promotion: the override exists to hold a *challenger* in
            // place, and a demoted challenger must not keep the surface.
            clear_override(db, surface_id).await?;
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
    source: &str,
) -> Result<String> {
    let sql = sql_owned(
        db,
        "INSERT INTO experiment_decisions \
         (id, surface_id, day, decision, incumbent_arm, challenger_arm, lift, evidence, guardrail_name, \
          guardrail_value, consecutive, algorithms_before, algorithms_after, created_at, source) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, datetime('now'), ?14)"
            .into(),
        "INSERT INTO experiment_decisions \
         (id, surface_id, day, decision, incumbent_arm, challenger_arm, lift, evidence, guardrail_name, \
          guardrail_value, consecutive, algorithms_before, algorithms_after, created_at, source) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, now(), $14)"
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
                .bind(source)
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
                .bind(source)
                .execute(
                    db.postgres_pool()
                        .expect("postgres pool for postgres backend"),
                )
                .await?;
        }
    }
    Ok(id)
}

/// Install the algorithms a promotion won with, as an override over the configured set.
///
/// One row per surface, replaced in place: the surface runs one set at a time, and a history of overrides would leave the
/// reader to work out which one is live. The history lives in `experiment_decisions`, where a rollback reads it.
pub async fn install_override(
    db: &Database,
    surface_id: &str,
    algorithms: &[String],
    // `None` for an override an operator installs by hand: not every swap is argued for by a decision row, and inventing one
    // would put a promotion in the history that never happened.
    decision_id: Option<&str>,
) -> Result<()> {
    let sql = sql_owned(
        db,
        "INSERT INTO surface_algorithm_overrides (surface_id, algorithms, decision_id, created_at) \
         VALUES (?1, ?2, ?3, datetime('now')) \
         ON CONFLICT (surface_id) DO UPDATE SET algorithms = excluded.algorithms, \
             decision_id = excluded.decision_id, created_at = excluded.created_at"
            .into(),
        "INSERT INTO surface_algorithm_overrides (surface_id, algorithms, decision_id, created_at) \
         VALUES ($1, $2, $3, now()) \
         ON CONFLICT (surface_id) DO UPDATE SET algorithms = EXCLUDED.algorithms, \
             decision_id = EXCLUDED.decision_id, created_at = EXCLUDED.created_at"
            .into(),
    );
    let list = serde_json::to_string(algorithms)?;
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(surface_id)
                .bind(&list)
                .bind(decision_id)
                .execute(db.sqlite_pool().expect("sqlite pool for sqlite backend"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(surface_id)
                .bind(&list)
                .bind(decision_id)
                .execute(
                    db.postgres_pool()
                        .expect("postgres pool for postgres backend"),
                )
                .await?;
        }
    }
    Ok(())
}

/// The algorithms a promoted experiment installed for this surface, if any.
///
/// The read path takes this in precedence over the configured set — that precedence is the whole point of the table, and it
/// belongs to the caller that builds the engine list, not here.
pub async fn override_for(db: &Database, surface_id: &str) -> Result<Option<Vec<String>>> {
    let sql = match db.backend() {
        Backend::Sqlite => {
            "SELECT algorithms FROM surface_algorithm_overrides WHERE surface_id = ?1"
        }
        Backend::Postgres => {
            "SELECT algorithms FROM surface_algorithm_overrides WHERE surface_id = $1"
        }
    };
    let row: Option<String> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(sql)
                .bind(surface_id)
                .fetch_optional(db.sqlite_pool().expect("sqlite pool for sqlite backend"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(sql)
                .bind(surface_id)
                .fetch_optional(
                    db.postgres_pool()
                        .expect("postgres pool for postgres backend"),
                )
                .await?
        }
    };
    Ok(row.and_then(|text| serde_json::from_str(&text).ok()))
}

/// Remove a surface's override, returning the algorithms to the configured set. Rollback is this call plus the row that
/// says who asked for it.
pub async fn clear_override(db: &Database, surface_id: &str) -> Result<()> {
    let sql = match db.backend() {
        Backend::Sqlite => "DELETE FROM surface_algorithm_overrides WHERE surface_id = ?1",
        Backend::Postgres => "DELETE FROM surface_algorithm_overrides WHERE surface_id = $1",
    };
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(sql)
                .bind(surface_id)
                .execute(db.sqlite_pool().expect("sqlite pool for sqlite backend"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(sql)
                .bind(surface_id)
                .execute(
                    db.postgres_pool()
                        .expect("postgres pool for postgres backend"),
                )
                .await?;
        }
    }
    Ok(())
}

/// The guardrail readings the platform can actually measure, one entry per day, oldest first (M64-04).
///
/// The requirement asks for "every guardrail in bounds" and the tree had no guardrail source at all, which made the clause
/// vacuously true and the demotion path unreachable in production. These two are derived from data the snapshots already
/// hold, so they need no new table and no invented metric:
///
/// * **conversion_rate** — the challenger's events/impressions must stay at or above half the incumbent's rate. A challenger
///   that ranks beautifully and converts half as well is the failure this experiment exists to catch, and no sample size
///   makes it acceptable.
/// * **traffic_share** — the challenger's impressions must stay at or above half the incumbent's. A set that quietly hides
///   the surface is a regression that the conversion rate cannot see, because it has fewer chances to convert.
///
/// Ratios rather than absolute values, deliberately: an absolute floor cannot tell a quiet day from a broken arm, and this
/// platform's traffic is not steady enough to pretend otherwise. A day where the incumbent had no impressions produces **no
/// reading** rather than a passing one — nothing was measured, and a fabricated pass is worse than a missing day.
pub async fn guardrail_history(
    db: &Database,
    surface_id: &str,
    incumbent_arm: i64,
    challenger_arm: i64,
) -> Result<Vec<Vec<GuardrailReading>>> {
    // One row per arm per day, oldest first: the history `decide` reads is ordered because consecutive breaches are a chain.
    let sql = match db.backend() {
        Backend::Sqlite => {
            "SELECT day, arm, impressions, events FROM experiment_snapshots \
             WHERE surface_id = ?1 AND arm IN (?2, ?3) ORDER BY day, arm"
        }
        Backend::Postgres => {
            "SELECT day, arm, impressions, events FROM experiment_snapshots \
             WHERE surface_id = $1 AND arm IN ($2, $3) ORDER BY day, arm"
        }
    };
    type DayRow = (String, i64, i64, i64);
    let rows: Vec<DayRow> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(sql)
                .bind(surface_id)
                .bind(incumbent_arm)
                .bind(challenger_arm)
                .fetch_all(db.sqlite_pool().expect("sqlite pool for sqlite backend"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(sql)
                .bind(surface_id)
                .bind(incumbent_arm)
                .bind(challenger_arm)
                .fetch_all(
                    db.postgres_pool()
                        .expect("postgres pool for postgres backend"),
                )
                .await?
        }
    };

    // Group by day, keeping SQL's ordering rather than a map: a BTreeMap would sort by day string, which happens to be
    // right for `YYYY-MM-DD` and wrong the first time anyone changes the format.
    let mut history: Vec<Vec<GuardrailReading>> = Vec::new();
    let mut current_day: Option<String> = None;
    let mut incumbent: Option<(i64, i64)> = None;
    let mut challenger: Option<(i64, i64)> = None;

    for (day, arm, impressions, events) in rows {
        if current_day.as_deref() != Some(day.as_str()) {
            push_day(&mut history, incumbent, challenger);
            current_day = Some(day);
            incumbent = None;
            challenger = None;
        }
        if arm == incumbent_arm {
            incumbent = Some((impressions, events));
        } else {
            challenger = Some((impressions, events));
        }
    }
    push_day(&mut history, incumbent, challenger);
    Ok(history)
}

fn rate(impressions: i64, events: i64) -> f64 {
    if impressions <= 0 {
        0.0
    } else {
        events as f64 / impressions as f64
    }
}

/// Append one day's readings, or nothing when the day measured nothing.
fn push_day(
    history: &mut Vec<Vec<GuardrailReading>>,
    incumbent: Option<(i64, i64)>,
    challenger: Option<(i64, i64)>,
) {
    let (Some((inc_impressions, inc_events)), Some((ch_impressions, ch_events))) =
        (incumbent, challenger)
    else {
        return;
    };
    if inc_impressions <= 0 {
        return;
    }

    let mut readings = Vec::new();

    let conversion_ratio = rate(ch_impressions, ch_events) / rate(inc_impressions, inc_events);
    readings.push(GuardrailReading {
        name: "conversion_rate".to_owned(),
        // `rate(...)` is 0/0 on a day where the incumbent converted nothing; the ratio is then infinite and the guardrail
        // must not fail on that arithmetic, so a zero incumbent rate is a pass by definition (there is nothing to fall
        // below).
        in_bounds: rate(inc_impressions, inc_events) == 0.0 || conversion_ratio >= 0.5,
        value: conversion_ratio,
    });

    let traffic_ratio = ch_impressions as f64 / inc_impressions as f64;
    readings.push(GuardrailReading {
        name: "traffic_share".to_owned(),
        in_bounds: traffic_ratio >= 0.5,
        value: traffic_ratio,
    });

    history.push(readings);
}

/// Whether a decision was already recorded for this surface and day.
///
/// The sweep's idempotency key. A sweep that runs twice in one day must not evaluate twice: a second promotion for the same
/// day would be a second row arguing from the same numbers, and the console would show two promotions where one happened.
pub async fn decided_on(db: &Database, surface_id: &str, day: &str) -> Result<bool> {
    let sql = match db.backend() {
        Backend::Sqlite => {
            "SELECT count(*) FROM experiment_decisions WHERE surface_id = ?1 AND day = ?2"
        }
        Backend::Postgres => {
            "SELECT count(*) FROM experiment_decisions WHERE surface_id = $1 AND day = $2"
        }
    };
    let count: i64 = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(sql)
                .bind(surface_id)
                .bind(day)
                .fetch_one(db.sqlite_pool().expect("sqlite pool for sqlite backend"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(sql)
                .bind(surface_id)
                .bind(day)
                .fetch_one(
                    db.postgres_pool()
                        .expect("postgres pool for postgres backend"),
                )
                .await?
        }
    };
    Ok(count > 0)
}

/// Hold a surface: the sweep skips it and reports the hold rather than a decision.
pub async fn hold(db: &Database, surface_id: &str, reason: &str) -> Result<()> {
    let sql = sql_owned(
        db,
        "INSERT INTO experiment_holds (surface_id, reason, held_at) VALUES (?1, ?2, datetime('now')) \
         ON CONFLICT (surface_id) DO UPDATE SET reason = excluded.reason, held_at = excluded.held_at"
            .into(),
        "INSERT INTO experiment_holds (surface_id, reason, held_at) VALUES ($1, $2, now()) \
         ON CONFLICT (surface_id) DO UPDATE SET reason = EXCLUDED.reason, held_at = EXCLUDED.held_at"
            .into(),
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(surface_id)
                .bind(reason)
                .execute(db.sqlite_pool().expect("sqlite pool for sqlite backend"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(surface_id)
                .bind(reason)
                .execute(
                    db.postgres_pool()
                        .expect("postgres pool for postgres backend"),
                )
                .await?;
        }
    }
    Ok(())
}

/// Release a hold. Returns whether there was one, so the caller can say "released" rather than "released (nothing was held)".
pub async fn release(db: &Database, surface_id: &str) -> Result<bool> {
    let sql = match db.backend() {
        Backend::Sqlite => "DELETE FROM experiment_holds WHERE surface_id = ?1",
        Backend::Postgres => "DELETE FROM experiment_holds WHERE surface_id = $1",
    };
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(sql)
            .bind(surface_id)
            .execute(db.sqlite_pool().expect("sqlite pool for sqlite backend"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(sql)
            .bind(surface_id)
            .execute(
                db.postgres_pool()
                    .expect("postgres pool for postgres backend"),
            )
            .await?
            .rows_affected(),
    };
    Ok(affected > 0)
}

/// The reason a surface is held, if it is.
pub async fn hold_reason(db: &Database, surface_id: &str) -> Result<Option<String>> {
    let sql = match db.backend() {
        Backend::Sqlite => "SELECT reason FROM experiment_holds WHERE surface_id = ?1",
        Backend::Postgres => "SELECT reason FROM experiment_holds WHERE surface_id = $1",
    };
    Ok(match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(sql)
                .bind(surface_id)
                .fetch_optional(db.sqlite_pool().expect("sqlite pool for sqlite backend"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(sql)
                .bind(surface_id)
                .fetch_optional(
                    db.postgres_pool()
                        .expect("postgres pool for postgres backend"),
                )
                .await?
        }
    })
}

/// An operator's promotion or demotion, recorded exactly as the automatic one is.
///
/// The difference is the `source` column and nothing else: same table, same fields, same override installed afterwards. A
/// manual action that took a different path would produce a record the automatic machinery cannot read, and the console would
/// have to special-case it.
pub async fn record_manual(
    db: &Database,
    surface_id: &str,
    day: &str,
    decision: &str,
    algorithms_before: &[String],
    algorithms_after: &[String],
) -> Result<String> {
    let id = record(
        db,
        surface_id,
        day,
        decision,
        0,
        1,
        0.0,
        0.0,
        None,
        None,
        0,
        algorithms_before,
        algorithms_after,
        "manual",
    )
    .await?;
    if decision == "promote" {
        install_override(db, surface_id, algorithms_after, Some(&id)).await?;
    } else {
        clear_override(db, surface_id).await?;
    }
    Ok(id)
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
