//! M69-14 — the store half of a live badge: read the state, decide, write the state, and never lose a loss.
//!
//! The rule lives in `darkhold_domain::live_badge`. This module is the three writes that make it stick:
//!
//! 1. **the state**, upserted on `(account, badge)` so a second recompute repairs rather than duplicates;
//! 2. **the event**, written only on a change — a row per recompute would be a log of the scheduler, not of the badge;
//! 3. **the outbox event**, so whatever consumes events (a webhook, a federation actor) hears about a lost badge the same
//!    way it hears about anything else. It is enqueued rather than delivered inline because the sweep is already a long
//!    operation and a receiver's downtime must not be the sweep's problem.
//!
//! `since` is cleared on a loss rather than kept: "held since" is a claim about the present, and a row that kept the old
//! date would say a badge that is not held began on a date inside a period when it was not held. The history lives in the
//! events, which is what a history is for.

use anyhow::Result;
use sqlx::Row as _;

use darkhold_domain::live_badge::{self, Decision, LiveBadge, Transition};

use crate::{Backend, Database};

/// What one recompute did to one badge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The badge's slug.
    pub badge: String,
    /// Whether it is held after the recompute.
    pub held: bool,
    /// What changed.
    pub transition: Transition,
    /// The sentence stored with the event.
    pub reason: String,
}

impl Outcome {
    /// Whether this outcome is worth telling anybody about.
    #[must_use]
    pub fn is_change(&self) -> bool {
        self.transition.is_change()
    }
}

/// The stored state of one badge, as a reader's own page shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    /// The badge's slug.
    pub badge: String,
    /// The name, from this build's catalogue (the slug when this build no longer knows it).
    pub name: String,
    /// Whether it is held now.
    pub held: bool,
    /// When it was gained, if it is held.
    pub since: Option<String>,
    /// The affinity reading that decided it. `None` is "unmeasured", never zero.
    pub affinity_bp: Option<i64>,
    /// How much of the affinity weight was measured.
    pub evidence_bp: i64,
    /// When it was last checked.
    pub checked_at: String,
}

/// Whether the account holds a live badge.
async fn stored(db: &Database, account: &str, slug: &str) -> Result<bool> {
    let held = match db.backend() {
        Backend::Sqlite => sqlx::query_scalar::<_, i64>(
            "SELECT held FROM live_badge_state WHERE account_id = ? AND badge = ?",
        )
        .bind(account)
        .bind(slug)
        .fetch_optional(db.sqlite_pool().expect("sqlite"))
        .await?
        .map(|flag| flag != 0),
        Backend::Postgres => {
            sqlx::query_scalar::<_, bool>(
                "SELECT held FROM live_badge_state WHERE account_id = $1 AND badge = $2",
            )
            .bind(account)
            .bind(slug)
            .fetch_optional(db.postgres_pool().expect("postgres"))
            .await?
        }
    };
    Ok(held.unwrap_or(false))
}

fn bp_to_i64(value: Option<u16>) -> Option<i64> {
    value.map(i64::from)
}

/// Recompute one badge for one account, writing the state and — on a change — the event.
pub async fn recompute(
    db: &Database,
    account: &str,
    badge: &LiveBadge,
    affinity_bp: Option<u16>,
    evidence_bp: u16,
    now: &str,
) -> Result<Outcome> {
    let held_before = stored(db, account, badge.slug).await?;
    let decision: Decision = live_badge::decide(held_before, affinity_bp, evidence_bp, badge);

    // The state, in one statement. `since` is set when the badge is gained and cleared when it is lost, so a reader's
    // page cannot show "held since March" for a badge they do not hold.
    let since = if decision.held {
        Some(now.to_owned())
    } else {
        None
    };
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO live_badge_state
                    (account_id, badge, held, since, checked_at, affinity_bp, evidence_bp)
                 VALUES (?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT(account_id, badge) DO UPDATE SET
                    held = excluded.held,
                    since = CASE
                        WHEN excluded.held = 1 AND live_badge_state.held = 0 THEN excluded.since
                        WHEN excluded.held = 0 THEN NULL
                        ELSE live_badge_state.since
                    END,
                    checked_at = excluded.checked_at,
                    affinity_bp = excluded.affinity_bp,
                    evidence_bp = excluded.evidence_bp",
            )
            .bind(account)
            .bind(badge.slug)
            .bind(i64::from(decision.held))
            .bind(since.as_deref())
            .bind(now)
            .bind(bp_to_i64(affinity_bp))
            .bind(i64::from(evidence_bp))
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO live_badge_state
                    (account_id, badge, held, since, checked_at, affinity_bp, evidence_bp)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)
                 ON CONFLICT(account_id, badge) DO UPDATE SET
                    held = excluded.held,
                    since = CASE
                        WHEN excluded.held AND NOT live_badge_state.held THEN excluded.since
                        WHEN NOT excluded.held THEN NULL
                        ELSE live_badge_state.since
                    END,
                    checked_at = excluded.checked_at,
                    affinity_bp = excluded.affinity_bp,
                    evidence_bp = excluded.evidence_bp",
            )
            .bind(account)
            .bind(badge.slug)
            .bind(decision.held)
            .bind(since.as_deref())
            .bind(now)
            .bind(bp_to_i64(affinity_bp))
            .bind(i64::from(evidence_bp))
            .execute(db.postgres_pool().expect("postgres"))
            .await?;
        }
    }

    if decision.transition.is_change() {
        let id = uuid::Uuid::new_v4().to_string();
        match db.backend() {
            Backend::Sqlite => {
                sqlx::query(
                    "INSERT INTO live_badge_events
                        (id, account_id, badge, kind, affinity_bp, evidence_bp, reason, at)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(&id)
                .bind(account)
                .bind(badge.slug)
                .bind(decision.transition.name())
                .bind(bp_to_i64(affinity_bp))
                .bind(i64::from(evidence_bp))
                .bind(&decision.reason)
                .bind(now)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await?;
            }
            Backend::Postgres => {
                sqlx::query(
                    "INSERT INTO live_badge_events
                        (id, account_id, badge, kind, affinity_bp, evidence_bp, reason, at)
                     VALUES ($1::uuid, $2, $3, $4, $5, $6, $7, $8)",
                )
                .bind(&id)
                .bind(account)
                .bind(badge.slug)
                .bind(decision.transition.name())
                .bind(bp_to_i64(affinity_bp))
                .bind(i64::from(evidence_bp))
                .bind(&decision.reason)
                .bind(now)
                .execute(db.postgres_pool().expect("postgres"))
                .await?;
            }
        }

        // The outbox is where a *change* becomes deliverable. A gain and a loss get different topics, because a subscriber
        // that wants to congratulate and one that wants to explain are different subscribers.
        let topic = format!("badge.{}", decision.transition.name());
        let payload = serde_json::json!({
            "event_type": topic,
            "badge": badge.slug,
            "badge_name": badge.name,
            "account": account,
            "affinity_bp": affinity_bp,
            "evidence_bp": evidence_bp,
            "reason": decision.reason,
        });
        // A failure to enqueue must not lose the badge change: the state and the event row are already written, and the
        // outbox is a delivery mechanism. Logged rather than propagated, so a broken outbox costs notifications and never
        // a reader's badge.
        if let Err(error) =
            crate::outbox::enqueue(db, &topic, &payload.to_string(), Some(&id)).await
        {
            tracing::warn!(%error, %topic, "the badge transition was not enqueued");
        }
    }

    Ok(Outcome {
        badge: badge.slug.to_owned(),
        held: decision.held,
        transition: decision.transition,
        reason: decision.reason,
    })
}

/// Recompute every live badge for one account.
pub async fn recompute_all(
    db: &Database,
    account: &str,
    affinity_bp: Option<u16>,
    evidence_bp: u16,
    now: &str,
) -> Result<Vec<Outcome>> {
    let mut outcomes = Vec::with_capacity(live_badge::LIVE.len());
    for badge in live_badge::LIVE {
        outcomes.push(recompute(db, account, badge, affinity_bp, evidence_bp, now).await?);
    }
    Ok(outcomes)
}

/// Every live badge's stored state for one account, including badges this build knows of but has never decided for them.
///
/// A badge that has never been decided is reported as not held with no `since`, rather than being absent: a page that
/// lists only rows would appear to change shape when the sweep first runs, and "we have not looked at you yet" is itself
/// worth saying.
pub async fn for_account(db: &Database, account: &str) -> Result<Vec<State>> {
    let rows: Vec<State> = match db.backend() {
        Backend::Sqlite => {
            let raw = sqlx::query(
                "SELECT badge, held, since, checked_at, affinity_bp, evidence_bp
                   FROM live_badge_state WHERE account_id = ?",
            )
            .bind(account)
            .fetch_all(db.sqlite_pool().expect("sqlite"))
            .await?;
            raw.iter()
                .map(|row| State {
                    badge: row.get::<String, _>("badge"),
                    name: String::new(),
                    held: row.get::<i64, _>("held") != 0,
                    since: row.get::<Option<String>, _>("since"),
                    affinity_bp: row.get::<Option<i64>, _>("affinity_bp"),
                    evidence_bp: row.get::<i64, _>("evidence_bp"),
                    checked_at: row.get::<String, _>("checked_at"),
                })
                .collect()
        }
        Backend::Postgres => {
            let raw = sqlx::query(
                "SELECT badge, held, since, checked_at, CAST(affinity_bp AS BIGINT) AS affinity_bp,
                        CAST(evidence_bp AS BIGINT) AS evidence_bp
                   FROM live_badge_state WHERE account_id = $1",
            )
            .bind(account)
            .fetch_all(db.postgres_pool().expect("postgres"))
            .await?;
            raw.iter()
                .map(|row| State {
                    badge: row.get::<String, _>("badge"),
                    name: String::new(),
                    held: row.get::<bool, _>("held"),
                    since: row.get::<Option<String>, _>("since"),
                    affinity_bp: row.get::<Option<i64>, _>("affinity_bp"),
                    evidence_bp: row.get::<i64, _>("evidence_bp"),
                    checked_at: row.get::<String, _>("checked_at"),
                })
                .collect()
        }
    };

    let mut states: Vec<State> = live_badge::LIVE
        .iter()
        .map(|badge| {
            let stored = rows.iter().find(|row| row.badge == badge.slug);
            State {
                badge: badge.slug.to_owned(),
                name: badge.name.to_owned(),
                held: stored.is_some_and(|row| row.held),
                since: stored.and_then(|row| row.since.clone()),
                affinity_bp: stored.and_then(|row| row.affinity_bp),
                evidence_bp: stored.map_or(0, |row| row.evidence_bp),
                checked_at: stored.map(|row| row.checked_at.clone()).unwrap_or_default(),
            }
        })
        .collect();
    // A stored badge this build no longer knows is kept in the list rather than dropped: it is held, and hiding it would
    // be the same silence this feature exists to avoid.
    for row in rows {
        if !states.iter().any(|state| state.badge == row.badge) {
            states.push(State {
                name: row.badge.clone(),
                ..row
            });
        }
    }
    Ok(states)
}

/// The change history for one account, newest first.
pub async fn events(db: &Database, account: &str, limit: i64) -> Result<Vec<serde_json::Value>> {
    let limit = limit.clamp(1, 200);
    // The projection is written per arm: `SqliteRow` and `PgRow` are different types in SQLx, so a shared closure would
    // need `Decode` bounds that the two databases do not unify (the same rule as `two-engine-store-tests`).
    match db.backend() {
        Backend::Sqlite => {
            let rows = sqlx::query(
                "SELECT badge, kind, affinity_bp, evidence_bp, reason, at
                   FROM live_badge_events WHERE account_id = ?
                  ORDER BY seq DESC LIMIT ?",
            )
            .bind(account)
            .bind(limit)
            .fetch_all(db.sqlite_pool().expect("sqlite"))
            .await?;
            Ok(rows
                .iter()
                .map(|row| {
                    serde_json::json!({
                        "badge": row.get::<String, _>("badge"),
                        "kind": row.get::<String, _>("kind"),
                        "affinity_bp": row.get::<Option<i64>, _>("affinity_bp"),
                        "evidence_bp": row.get::<i64, _>("evidence_bp"),
                        "reason": row.get::<String, _>("reason"),
                        "at": row.get::<String, _>("at"),
                    })
                })
                .collect())
        }
        Backend::Postgres => {
            let rows = sqlx::query(
                "SELECT badge, kind, CAST(affinity_bp AS BIGINT) AS affinity_bp,
                        CAST(evidence_bp AS BIGINT) AS evidence_bp, reason, at
                   FROM live_badge_events WHERE account_id = $1
                  ORDER BY seq DESC LIMIT $2",
            )
            .bind(account)
            .bind(limit)
            .fetch_all(db.postgres_pool().expect("postgres"))
            .await?;
            Ok(rows
                .iter()
                .map(|row| {
                    serde_json::json!({
                        "badge": row.get::<String, _>("badge"),
                        "kind": row.get::<String, _>("kind"),
                        "affinity_bp": row.get::<Option<i64>, _>("affinity_bp"),
                        "evidence_bp": row.get::<i64, _>("evidence_bp"),
                        "reason": row.get::<String, _>("reason"),
                        "at": row.get::<String, _>("at"),
                    })
                })
                .collect())
        }
    }
}
