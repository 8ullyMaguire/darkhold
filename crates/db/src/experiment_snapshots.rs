//! Per-arm daily snapshots (M64-03).
//!
//! A promotion reads a snapshot, never the raw log. The difference matters at exactly one moment — the decision — and it
//! is the moment everything else in the experiment platform exists for: a number read from `recommendation_slots` changes
//! while the argument about it happens, and afterwards nobody can say what the arm looked like when the decision was
//! taken. One row per `(surface, arm, day)`, written idempotently, is that record.
//!
//! # What the columns mean, fixed here rather than per consumer
//!
//! * **impressions** — slots served in the day whose `(pseud, surface)` has an arm. A slot with no assignment is not an
//!   observation of an arm, so it is not counted; the join is the definition.
//! * **readers** — distinct pseuds behind those impressions.
//! * **guest_readers** — the distinct pseuds whose account carries `is_guest`. This is a guardrail input, not colour: the
//!   requirement is that one bot cannot decide a winner, and "how much of this arm is guest traffic" must be answerable
//!   from the snapshot rather than recomputed later against a log that has moved on.
//! * **events** — impressions that were followed by the choice event (`recommendation_slots.chosen_at` set). Named for
//!   what is stored, so nobody has to guess whether it means "opens" or "any engagement".
//!
//! # What is deliberately absent
//!
//! A significance test. Whether an arm has won is a sequential test over these counts with guardrails in bounds
//! (M64-04), and storing a p-value in a nightly row would freeze a method into a schema. The snapshot stores counts and
//! the promotion computes from them.

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::Database;

/// One arm's numbers for one day.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArmSnapshot {
    pub surface_id: String,
    pub arm: i64,
    pub day: String,
    pub impressions: i64,
    pub readers: i64,
    pub guest_readers: i64,
    pub events: i64,
}

/// Compute and store the snapshot for one day, replacing any earlier run of the same day.
///
/// `day` is `YYYY-MM-DD`; the window is that calendar date in UTC, `[day, day+1)`. A re-run is a repair, not a duplicate:
/// `ON CONFLICT (surface_id, arm, day) DO UPDATE` means the job can be run twice after fixing a bug upstream and the day
/// ends up right rather than doubled.
pub async fn snapshot_day(db: &Database, day: &str) -> Result<i64> {
    if day.len() != 10 {
        anyhow::bail!("a snapshot day is YYYY-MM-DD, not {day:?}");
    }
    let start = format!("{day}T00:00:00");
    // The window end is the next midnight, written as the same date with a day added by SQL so the caller does not have
    // to know about month lengths. `date(?, '+1 day')` and `$1::date + 1` are the two dialects' spellings of that.
    let sql = match db.backend() {
        crate::Backend::Sqlite => {
            "INSERT INTO experiment_snapshots \
             (id, surface_id, arm, day, impressions, readers, guest_readers, events, computed_at) \
             SELECT lower(hex(randomblob(16))), s.mechanism, a.bucket, ?1, \
                    count(*), \
                    count(DISTINCT s.pseud_id), \
                    count(DISTINCT CASE WHEN ac.is_guest THEN s.pseud_id END), \
                    count(CASE WHEN s.chosen_at IS NOT NULL THEN 1 END), \
                    datetime('now') \
             FROM recommendation_slots s \
             JOIN experiment_assignments a ON a.pseud_id = s.pseud_id AND a.surface_id = s.mechanism \
             JOIN pseuds p ON p.id = s.pseud_id \
             JOIN accounts ac ON ac.id = p.account_id \
             WHERE s.created_at >= ?2 AND s.created_at < datetime(?2, '+1 day') \
             GROUP BY s.mechanism, a.bucket \
             ON CONFLICT (surface_id, arm, day) DO UPDATE SET \
                 impressions = excluded.impressions, readers = excluded.readers, \
                 guest_readers = excluded.guest_readers, events = excluded.events, \
                 computed_at = excluded.computed_at"
        }
        crate::Backend::Postgres => {
            "INSERT INTO experiment_snapshots \
             (id, surface_id, arm, day, impressions, readers, guest_readers, events, computed_at) \
             SELECT gen_random_uuid(), s.mechanism, a.bucket, $1, \
                    count(*), \
                    count(DISTINCT s.pseud_id), \
                    count(DISTINCT CASE WHEN ac.is_guest THEN s.pseud_id END), \
                    count(CASE WHEN s.chosen_at IS NOT NULL THEN 1 END), \
                    now() \
             FROM recommendation_slots s \
             JOIN experiment_assignments a ON a.pseud_id = s.pseud_id AND a.surface_id = s.mechanism \
             JOIN pseuds p ON p.id = s.pseud_id \
             JOIN accounts ac ON ac.id = p.account_id \
             WHERE s.created_at >= $2::timestamptz AND s.created_at < ($2::timestamptz + interval '1 day') \
             GROUP BY s.mechanism, a.bucket \
             ON CONFLICT (surface_id, arm, day) DO UPDATE SET \
                 impressions = excluded.impressions, readers = excluded.readers, \
                 guest_readers = excluded.guest_readers, events = excluded.events, \
                 computed_at = excluded.computed_at"
        }
    };

    let written = match db.backend() {
        crate::Backend::Sqlite => sqlx::query(sql)
            .bind(day)
            .bind(&start)
            .execute(db.sqlite_pool().expect("sqlite pool for sqlite backend"))
            .await?
            .rows_affected(),
        crate::Backend::Postgres => sqlx::query(sql)
            .bind(day)
            .bind(&start)
            .execute(
                db.postgres_pool()
                    .expect("postgres pool for postgres backend"),
            )
            .await?
            .rows_affected(),
    };
    Ok(i64::try_from(written).unwrap_or(i64::MAX))
}

/// Every arm of every surface for one day, ordered so the surfaces group together.
pub async fn day_snapshots(db: &Database, day: &str) -> Result<Vec<ArmSnapshot>> {
    let sql = match db.backend() {
        crate::Backend::Sqlite => {
            "SELECT surface_id, arm, day, impressions, readers, guest_readers, events \
             FROM experiment_snapshots WHERE day = ?1 ORDER BY surface_id, arm"
        }
        crate::Backend::Postgres => {
            "SELECT surface_id, arm, day, impressions, readers, guest_readers, events \
             FROM experiment_snapshots WHERE day = $1 ORDER BY surface_id, arm"
        }
    };
    let mut snapshots = Vec::new();
    match db.backend() {
        crate::Backend::Sqlite => {
            let rows: Vec<(String, i64, String, i64, i64, i64, i64)> = sqlx::query_as(sql)
                .bind(day)
                .fetch_all(db.sqlite_pool().expect("sqlite pool for sqlite backend"))
                .await?;
            for row in rows {
                snapshots.push(ArmSnapshot {
                    surface_id: row.0,
                    arm: row.1,
                    day: row.2,
                    impressions: row.3,
                    readers: row.4,
                    guest_readers: row.5,
                    events: row.6,
                });
            }
        }
        crate::Backend::Postgres => {
            let rows: Vec<(String, i32, String, i64, i64, i64, i64)> = sqlx::query_as(sql)
                .bind(day)
                .fetch_all(
                    db.postgres_pool()
                        .expect("postgres pool for postgres backend"),
                )
                .await?;
            for row in rows {
                snapshots.push(ArmSnapshot {
                    surface_id: row.0,
                    arm: i64::from(row.1),
                    day: row.2,
                    impressions: row.3,
                    readers: row.4,
                    guest_readers: row.5,
                    events: row.6,
                });
            }
        }
    }
    Ok(snapshots)
}

/// The most recent day that has a snapshot, so a caller that did not name one reads the last decision-grade numbers rather
/// than an empty answer for a day nobody has computed yet.
///
/// `None` means no snapshot has ever been written: an empty experiment platform and a platform whose job has not run are
/// different states, and the caller says which.
pub async fn latest_snapshots(db: &Database) -> Result<Option<Vec<ArmSnapshot>>> {
    let day_sql = "SELECT max(day) FROM experiment_snapshots";
    let day: Option<String> = match db.backend() {
        crate::Backend::Sqlite => {
            sqlx::query_scalar(day_sql)
                .fetch_one(db.sqlite_pool().expect("sqlite pool for sqlite backend"))
                .await?
        }
        crate::Backend::Postgres => {
            sqlx::query_scalar(day_sql)
                .fetch_one(
                    db.postgres_pool()
                        .expect("postgres pool for postgres backend"),
                )
                .await?
        }
    };
    match day {
        Some(day) => Ok(Some(day_snapshots(db, &day).await?)),
        None => Ok(None),
    }
}
