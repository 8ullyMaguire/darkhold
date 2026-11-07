//! M69-09 — the reader-affinity sweep: read the evidence, compute, store one row per reader.
//!
//! The arithmetic is `darkhold_domain::affinity` (weights, reweighting over present evidence, clamping). This module
//! gathers the three signals from tables the instance already keeps and writes the result.
//!
//! # The sweep, not a request
//!
//! Three joins over the whole readership do not belong in a page render, so affinity is a sweep: one call walks every
//! account with any evidence and upserts one row each. There is no scheduler in this tree, so the trigger is an
//! operator door and the row's ledger notes say so — a `tokio::interval` invented inside a handler would be a timer per
//! request multiplied by the process count.
//!
//! The write is an upsert on the account, so a second run **repairs** rather than doubles, and the report separates
//! `initialised` (first score for this reader) from `changed` (a score that moved). That distinction is what makes a
//! second run over unchanged data print zeros instead of claiming every reader drifted.
//!
//! # What evidence each signal reads
//!
//! * **favourite overlap** — the reader's bookmarked works against the works the operator has valued
//!   (`operator_affinities.affinity_bp >= 2500`), as Jaccard.
//! * **tag similarity** — cosine over tag counts: the reader's own works' tags against the valued works' tags. Counts,
//!   not sets, because a reader whose strongest interest is one the instance shares is aligned even if the individual
//!   works differ.
//! * **arena agreement** — of the ballot pairs both judged, the share where the same work won.

use std::collections::BTreeMap;

use anyhow::Result;
use serde_json::{json, Value};
use sqlx::Row as _;

use darkhold_domain::affinity::{self, Components};

use crate::{Backend, Database};

/// A work counts as "valued by the instance" at this operator affinity or above (25%).
///
/// A threshold rather than "any positive value": `set_operator_affinity` clamps to `-5000..=10000`, so a work at 1bp is
/// the operator shrugging, and treating a shrug as a value would make every holding count as overlap.
pub const VALUED_THRESHOLD_BP: i64 = 2_500;

/// One reader's new score, as the sweep reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The account.
    pub account_id: String,
    /// 0..=10000.
    pub basis_points: u16,
    /// How many signals carried evidence.
    pub signals: u8,
    /// `true` when this is the reader's first score.
    pub initialised: bool,
}

/// What one sweep did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// Readers scored for the first time.
    pub initialised: usize,
    /// Readers whose score moved.
    pub changed: usize,
    /// Readers whose score was recomputed to the same value.
    pub unchanged: usize,
    /// Candidates the sweep could not score because no signal had evidence: no row written, and counted in the report
    /// rather than silently skipped. (A reader with no bookmarks and no ballots never becomes a candidate; see
    /// `candidates`.)
    pub unscored: usize,
    /// The changes themselves, so a caller can log or display them without a second query.
    pub changes: Vec<Change>,
    /// Live badges this sweep awarded (M69-14).
    pub badges_gained: usize,
    /// Live badges this sweep took away. A loss is counted separately because it is the one that needs explaining.
    pub badges_lost: usize,
}

impl Report {
    /// Whether the sweep wrote anything.
    #[must_use]
    pub fn wrote(&self) -> bool {
        self.initialised + self.changed > 0
    }

    /// A JSON shape for the operator route.
    #[must_use]
    pub fn to_json(&self, scored: usize) -> Value {
        json!({
            "scored": scored,
            "initialised": self.initialised,
            "changed": self.changed,
            "unchanged": self.unchanged,
            "unscored": self.unscored,
            "badges_gained": self.badges_gained,
            "badges_lost": self.badges_lost,
            "changes": self.changes.iter().map(|c| json!({
                "account": c.account_id,
                "score_bp": c.basis_points,
                "signals": c.signals,
                "initialised": c.initialised,
            })).collect::<Vec<Value>>(),
            "note": "a second run over unchanged data reports zeros; a repeated non-zero 'changed' means the lookup is wrong",
        })
    }
}

/// The accounts the sweep should consider: anyone with a bookmark or a ballot.
///
/// Derived from the evidence tables rather than from `accounts`, so a reader who has never done anything is not a row
/// of zeros — they are nobody the sweep has anything to say about.
async fn candidates(db: &Database) -> Result<Vec<String>> {
    let rows: Vec<(String,)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(
                "SELECT account_id FROM bookmarks
             UNION SELECT account_id FROM arena_ballots
             ORDER BY account_id",
            )
            .fetch_all(db.sqlite_pool().expect("sqlite"))
            .await?
        }
        // Account ids are `uuid` on PostgreSQL: a plain `SELECT` decodes to `Uuid` and fails the `String` shape, which
        // is a runtime error on the engine the tests do not default to. The cast belongs in the statement.
        Backend::Postgres => {
            sqlx::query_as(
                "SELECT CAST(account_id AS TEXT) AS account_id FROM bookmarks
             UNION SELECT CAST(account_id AS TEXT) FROM arena_ballots
             ORDER BY account_id",
            )
            .fetch_all(db.postgres_pool().expect("postgres"))
            .await?
        }
    };
    Ok(rows.into_iter().map(|(account,)| account).collect())
}

/// The works the operator values.
async fn valued_works(db: &Database) -> Result<Vec<String>> {
    let rows: Vec<(String,)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as("SELECT work_id FROM operator_affinities WHERE affinity_bp >= ?")
                .bind(VALUED_THRESHOLD_BP)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(
                // `work_id` is uuid on PostgreSQL and text on SQLite; the sweep compares these as strings, so the cast
                // is in the statement rather than left to the decode (an `i64`-shaped intuition that decodes to a
                // runtime error on the engine the tests do not default to).
                "SELECT CAST(work_id AS TEXT) FROM operator_affinities WHERE CAST(affinity_bp AS BIGINT) >= $1",
            )
            .bind(VALUED_THRESHOLD_BP)
            .fetch_all(db.postgres_pool().expect("postgres"))
            .await?
        }
    };
    Ok(rows.into_iter().map(|(work,)| work).collect())
}

/// The reader's collected works: bookmarks that name a work.
async fn collected_works(db: &Database, account: &str) -> Result<Vec<String>> {
    let rows: Vec<(String,)> = match db.backend() {
        Backend::Sqlite => sqlx::query_as(
            "SELECT DISTINCT subject_id FROM bookmarks WHERE account_id = ? AND subject_type = 'work'",
        )
        .bind(account)
        .fetch_all(db.sqlite_pool().expect("sqlite"))
        .await?,
        Backend::Postgres => sqlx::query_as(
            "SELECT DISTINCT CAST(subject_id AS TEXT) AS subject_id FROM bookmarks
             WHERE account_id = $1::uuid AND subject_type = 'work'",
        )
        .bind(account)
        .fetch_all(db.postgres_pool().expect("postgres"))
        .await?,
    };
    Ok(rows.into_iter().map(|(work,)| work).collect())
}

/// Tag counts over a set of works.
///
/// `IN (?, ?, …)` is built from the caller's ids because SQLx has no array binding that works the same on both
/// engines. An empty set returns an empty map without a query: `IN ()` is a syntax error on one engine and a false
/// predicate on the other, and either way the answer is known.
async fn tag_counts(db: &Database, works: &[String]) -> Result<BTreeMap<String, f64>> {
    if works.is_empty() {
        return Ok(BTreeMap::new());
    }
    let mut counts = BTreeMap::new();
    match db.backend() {
        Backend::Sqlite => {
            let placeholders = vec!["?"; works.len()].join(", ");
            let sql = format!(
                "SELECT node_id, COUNT(*) AS n FROM work_tags WHERE work_id IN ({placeholders}) GROUP BY node_id"
            );
            let mut query = sqlx::query(&sql);
            for work in works {
                query = query.bind(work);
            }
            for row in query.fetch_all(db.sqlite_pool().expect("sqlite")).await? {
                counts.insert(
                    row.get::<String, _>("node_id"),
                    row.get::<i64, _>("n") as f64,
                );
            }
        }
        Backend::Postgres => {
            // `$n::uuid`, because `work_tags.work_id` is `uuid` here and a text parameter would fail the comparison
            // rather than the decode — a different error from the same cause.
            let placeholders = (1..=works.len())
                .map(|n| format!("${n}::uuid"))
                .collect::<Vec<String>>()
                .join(", ");
            let sql = format!(
                "SELECT node_id, CAST(COUNT(*) AS BIGINT) AS n FROM work_tags WHERE work_id IN ({placeholders}) GROUP BY node_id"
            );
            let mut query = sqlx::query(&sql);
            for work in works {
                query = query.bind(work);
            }
            for row in query
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?
            {
                counts.insert(
                    row.get::<String, _>("node_id"),
                    row.get::<i64, _>("n") as f64,
                );
            }
        }
    }
    Ok(counts)
}

/// Arena agreement with the operator's own ballots.
///
/// Returns `(shared, agreed)`. A shared pair is a ballot of the reader's whose best/worst pair the operator also judged
/// — with the same winner, or the mirror image (the operator may have seen the pair in the other order). Both count as
/// agreement about the *relation between the two works*, which is what a ballot states.
async fn arena_shared(db: &Database, account: &str, operator: &str) -> Result<(usize, usize)> {
    let reader: Vec<(String, String)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(
                "SELECT best_work_id, worst_work_id FROM arena_ballots WHERE account_id = ?",
            )
            .bind(account)
            .fetch_all(db.sqlite_pool().expect("sqlite"))
            .await?
        }
        Backend::Postgres => {
            sqlx::query_as(
                "SELECT CAST(best_work_id AS TEXT) AS best_work_id, CAST(worst_work_id AS TEXT) AS worst_work_id
                 FROM arena_ballots WHERE account_id = $1::uuid",
            )
            .bind(account)
            .fetch_all(db.postgres_pool().expect("postgres"))
            .await?
        }
    };
    let theirs: Vec<(String, String)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(
                "SELECT best_work_id, worst_work_id FROM arena_ballots WHERE account_id = ?",
            )
            .bind(operator)
            .fetch_all(db.sqlite_pool().expect("sqlite"))
            .await?
        }
        Backend::Postgres => {
            sqlx::query_as(
                "SELECT CAST(best_work_id AS TEXT) AS best_work_id, CAST(worst_work_id AS TEXT) AS worst_work_id
                 FROM arena_ballots WHERE account_id = $1::uuid",
            )
            .bind(operator)
            .fetch_all(db.postgres_pool().expect("postgres"))
            .await?
        }
    };

    let mut shared = 0;
    let mut agreed = 0;
    for (best, worst) in &reader {
        for (their_best, their_worst) in &theirs {
            let same = best == their_best && worst == their_worst;
            let mirrored = best == their_worst && worst == their_best;
            if same || mirrored {
                shared += 1;
                agreed += 1;
                break;
            }
        }
    }
    Ok((shared, agreed))
}

/// Recompute every reader's affinity.
///
/// `operator` is the account whose taste the score is measured against. When there is none — an instance with no
/// designated operator — the sweep still runs and reports `unscored` for everyone: the honest answer is that there is
/// nothing to be close to, not a score of zero for the whole readership.
pub async fn recompute_all(
    db: &Database,
    operator: Option<&str>,
    now: &str,
    live_badges: bool,
) -> Result<Report> {
    let mut report = Report::default();
    let valued = valued_works(db).await?;
    let candidate_ids = candidates(db).await?;

    if valued.is_empty() || operator.is_none() {
        report.unscored = candidate_ids.len();
        return Ok(report);
    }
    let operator = operator.expect("checked above");
    let instance_tags = tag_counts(db, &valued).await?;

    for account in candidate_ids {
        let collected = collected_works(db, &account).await?;
        let reader_tags = tag_counts(db, &collected).await?;
        let (shared, agreed) = arena_shared(db, &account, operator).await?;

        let components = Components {
            favourite_overlap: affinity::jaccard(&collected, &valued),
            tag_similarity: affinity::cosine(&reader_tags, &instance_tags),
            arena_agreement: affinity::arena_agreement(shared, agreed),
        };
        let Some(score) = affinity::score(&components) else {
            report.unscored += 1;
            continue;
        };

        let stored = stored_bp(db, &account).await?;
        write(db, &account, &score, &components, now).await?;

        // M69-14: a live badge is decided by the reading that was just written, in the same pass — "recomputed with
        // affinity" is the requirement, and a second sweep would be a second thing to forget to run. Off unless the
        // instance enabled it, and when off nothing is decided at all: a state row would be the instance measuring a
        // reader for a badge it has told nobody about.
        let outcomes = if live_badges {
            crate::live_badges::recompute_all(
                db,
                &account,
                Some(score.basis_points),
                score.evidence_bp,
                now,
            )
            .await?
        } else {
            Vec::new()
        };
        for outcome in outcomes {
            match outcome.transition {
                darkhold_domain::live_badge::Transition::Gained => report.badges_gained += 1,
                darkhold_domain::live_badge::Transition::Lost => report.badges_lost += 1,
                _ => {}
            }
        }

        let changed = match stored {
            None => {
                report.initialised += 1;
                true
            }
            Some(previous) if previous != i64::from(score.basis_points) => {
                report.changed += 1;
                true
            }
            Some(_) => {
                report.unchanged += 1;
                false
            }
        };
        if changed {
            report.changes.push(Change {
                account_id: account,
                basis_points: score.basis_points,
                signals: score.signals,
                initialised: stored.is_none(),
            });
        }
    }

    Ok(report)
}

/// The stored score for a reader, if they have one.
async fn stored_bp(db: &Database, account: &str) -> Result<Option<i64>> {
    Ok(match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar("SELECT score_bp FROM reader_affinity WHERE account_id = ?")
                .bind(account)
                .fetch_optional(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(
                "SELECT CAST(score_bp AS BIGINT) FROM reader_affinity WHERE account_id = $1",
            )
            .bind(account)
            .fetch_optional(db.postgres_pool().expect("postgres"))
            .await?
        }
    })
}

/// Upsert one reader's score with its components.
///
/// `DO UPDATE` rather than `DO NOTHING`: a repair run after an outage must absorb new evidence, and `DO NOTHING` turns a
/// sweep into a one-shot that silently stops updating the moment a row exists.
async fn write(
    db: &Database,
    account: &str,
    score: &affinity::Score,
    components: &Components,
    now: &str,
) -> Result<()> {
    let bp = |value: Option<f64>| -> Option<i64> {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        value.map(|v| (v.clamp(0.0, 1.0) * 10_000.0).round() as i64)
    };
    let (favourite, tags, arena) = (
        bp(components.favourite_overlap),
        bp(components.tag_similarity),
        bp(components.arena_agreement),
    );
    let score_bp = i64::from(score.basis_points);
    let evidence_bp = i64::from(score.evidence_bp);
    let signals = i64::from(score.signals);

    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO reader_affinity
                    (account_id, score_bp, evidence_bp, signals, favourite_overlap_bp, tag_similarity_bp, arena_agreement_bp, computed_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT(account_id) DO UPDATE SET
                    score_bp = excluded.score_bp,
                    evidence_bp = excluded.evidence_bp,
                    signals = excluded.signals,
                    favourite_overlap_bp = excluded.favourite_overlap_bp,
                    tag_similarity_bp = excluded.tag_similarity_bp,
                    arena_agreement_bp = excluded.arena_agreement_bp,
                    computed_at = excluded.computed_at",
            )
            .bind(account)
            .bind(score_bp)
            .bind(evidence_bp)
            .bind(signals)
            .bind(favourite)
            .bind(tags)
            .bind(arena)
            .bind(now)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO reader_affinity
                    (account_id, score_bp, evidence_bp, signals, favourite_overlap_bp, tag_similarity_bp, arena_agreement_bp, computed_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                 ON CONFLICT(account_id) DO UPDATE SET
                    score_bp = excluded.score_bp,
                    evidence_bp = excluded.evidence_bp,
                    signals = excluded.signals,
                    favourite_overlap_bp = excluded.favourite_overlap_bp,
                    tag_similarity_bp = excluded.tag_similarity_bp,
                    arena_agreement_bp = excluded.arena_agreement_bp,
                    computed_at = excluded.computed_at",
            )
            .bind(account)
            .bind(score_bp)
            .bind(evidence_bp)
            .bind(signals)
            .bind(favourite)
            .bind(tags)
            .bind(arena)
            .bind(now)
            .execute(db.postgres_pool().expect("postgres"))
            .await?;
        }
    }
    Ok(())
}

/// One reader's stored affinity, with its components.
///
/// `None` means "no score", which is different from a score of zero: a reader with no evidence has not been measured,
/// and telling them they are 0% aligned would be a judgement the instance never made.
pub async fn for_account(db: &Database, account: &str) -> Result<Option<Value>> {
    // The two arms build the same JSON from two different row types. `SqliteRow` and `PgRow` are distinct types in
    // SQLx, so the projection lives inside each arm; a shared `Row` binding is the bug `two-engine-store-tests` warns
    // about, and the same shape appears in every module that reads both.
    match db.backend() {
        Backend::Sqlite => {
            let row = sqlx::query(
                "SELECT score_bp, evidence_bp, signals, favourite_overlap_bp, tag_similarity_bp, arena_agreement_bp, computed_at
                 FROM reader_affinity WHERE account_id = ?",
            )
            .bind(account)
            .fetch_optional(db.sqlite_pool().expect("sqlite"))
            .await?;
            Ok(row.map(|r| {
                json!({
                    "score_bp": r.get::<i64, _>("score_bp"),
                    "evidence_bp": r.get::<i64, _>("evidence_bp"),
                    "signals": r.get::<i64, _>("signals"),
                    "favourite_overlap_bp": r.get::<Option<i64>, _>("favourite_overlap_bp"),
                    "tag_similarity_bp": r.get::<Option<i64>, _>("tag_similarity_bp"),
                    "arena_agreement_bp": r.get::<Option<i64>, _>("arena_agreement_bp"),
                    "computed_at": r.get::<String, _>("computed_at"),
                })
            }))
        }
        Backend::Postgres => {
            let row = sqlx::query(
                "SELECT CAST(score_bp AS BIGINT) AS score_bp, CAST(evidence_bp AS BIGINT) AS evidence_bp, CAST(signals AS BIGINT) AS signals,
                        CAST(favourite_overlap_bp AS BIGINT) AS favourite_overlap_bp,
                        CAST(tag_similarity_bp AS BIGINT) AS tag_similarity_bp,
                        CAST(arena_agreement_bp AS BIGINT) AS arena_agreement_bp, computed_at
                 FROM reader_affinity WHERE account_id = $1",
            )
            .bind(account)
            .fetch_optional(db.postgres_pool().expect("postgres"))
            .await?;
            Ok(row.map(|r| {
                json!({
                    "score_bp": r.get::<i64, _>("score_bp"),
                    "evidence_bp": r.get::<i64, _>("evidence_bp"),
                    "signals": r.get::<i64, _>("signals"),
                    "favourite_overlap_bp": r.get::<Option<i64>, _>("favourite_overlap_bp"),
                    "tag_similarity_bp": r.get::<Option<i64>, _>("tag_similarity_bp"),
                    "arena_agreement_bp": r.get::<Option<i64>, _>("arena_agreement_bp"),
                    "computed_at": r.get::<String, _>("computed_at"),
                })
            }))
        }
    }
}

/// The instance's own view: every scored reader, highest first.
pub async fn ranked(db: &Database, limit: i64) -> Result<Vec<Value>> {
    let limit = limit.clamp(1, 1_000);
    match db.backend() {
        Backend::Sqlite => {
            let rows = sqlx::query(
                "SELECT account_id, score_bp, evidence_bp, signals, computed_at FROM reader_affinity
                 ORDER BY score_bp DESC, account_id LIMIT ?",
            )
            .bind(limit)
            .fetch_all(db.sqlite_pool().expect("sqlite"))
            .await?;
            Ok(rows
                .iter()
                .map(|r| {
                    json!({
                        "account": r.get::<String, _>("account_id"),
                        "score_bp": r.get::<i64, _>("score_bp"),
                        "evidence_bp": r.get::<i64, _>("evidence_bp"),
                        "signals": r.get::<i64, _>("signals"),
                        "computed_at": r.get::<String, _>("computed_at"),
                    })
                })
                .collect())
        }
        Backend::Postgres => {
            let rows = sqlx::query(
                "SELECT account_id, CAST(score_bp AS BIGINT) AS score_bp, CAST(evidence_bp AS BIGINT) AS evidence_bp, CAST(signals AS BIGINT) AS signals, computed_at
                 FROM reader_affinity ORDER BY score_bp DESC, account_id LIMIT $1",
            )
            .bind(limit)
            .fetch_all(db.postgres_pool().expect("postgres"))
            .await?;
            Ok(rows
                .iter()
                .map(|r| {
                    json!({
                        "account": r.get::<String, _>("account_id"),
                        "score_bp": r.get::<i64, _>("score_bp"),
                        "evidence_bp": r.get::<i64, _>("evidence_bp"),
                        "signals": r.get::<i64, _>("signals"),
                        "computed_at": r.get::<String, _>("computed_at"),
                    })
                })
                .collect())
        }
    }
}
