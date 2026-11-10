//! The rows behind the instance's taste (ADR 0016).
//!
//! Two questions, two statements: **who may contribute** (an account plus the facts a selector needs: curator flag, trust
//! level, held badges, measured affinity) and **what they contributed** (one normalised row per act: a rating, a
//! collection item, a saved search). The arithmetic lives in `polaris-taste-source`; this module only refuses to spell it
//! in SQL, because the two engines disagree about dates and the fold wants a float.
//!
//! # What is deliberately *not* here
//!
//! darkhold's `taste_vector` is a hand-authored dimension vector (`[taste].dimensions` with an `admin_target` per axis),
//! not evidence: it is the recommender's coordinate system, so folding it as evidence would add a subject nobody clicked
//! on to a profile of subjects. The instance's `admin_weight` stays where it is until the fold replaces the code path that
//! reads it, which is a separate row in the ledger.
//!
//! # A row is written in each dialect, not generated from one
//!
//! `rating.created_at` is TEXT, `collection_items.work_id` is TEXT against a uuid `work_tags.work_id`, and `is_public` is
//! BIGINT here — so the SQLite arm joins bare and the PostgreSQL arm casts, and the age arithmetic is `julianday` on one
//! engine and `EXTRACT(EPOCH …)` on the other. One statement pretending to be portable would work on the engine whose
//! timestamps happen to be ISO and return NULL ages on the other, which reads as "every contribution is recent".

use anyhow::Result;

use crate::{Backend, Database};

/// One account that may contribute, with the facts a selector reads.
#[derive(Debug, Clone, PartialEq)]
pub struct ContributorRow {
    /// The account id.
    pub id: String,
    /// Whether the account is a curator. Declared by the instance, so it is the same claim as `role:curator`.
    pub is_curator: bool,
    /// The instance's own trust ladder.
    pub trust_level: u8,
    /// Closeness to the instance, 0.0-1.0, from the affinity sweep. `None` is unmeasured, never zero.
    pub affinity: Option<f64>,
    /// How much evidence that closeness rests on, in tenths — so a selector can refuse a one-row measurement.
    pub affinity_evidence: u32,
    /// Live badges held right now (`attuned`, …). A live badge is held and lost, so this is read, not remembered.
    pub badges: Vec<String>,
}

/// One act, normalised into the estate's vocabulary.
#[derive(Debug, Clone, PartialEq)]
pub struct EvidenceRow {
    /// The account that acted.
    pub contributor: String,
    /// darkhold's own word for the act (`rating`, `collection`, `saved_search`); the app maps it onto a `Kind`.
    pub verb: String,
    /// What was acted on: a tag, or the text of a saved query.
    pub subject: String,
    /// 0.0-1.0; a rating is `stars / 5`.
    pub strength: f64,
    /// Days since the act. Negative and unparseable ages are clamped to 0 by the fold, not here.
    pub age_days: f64,
}

/// Every account that could contribute, with its selector facts.
pub async fn contributors(db: &Database) -> Result<Vec<ContributorRow>> {
    let sqlite = "SELECT a.id, CASE WHEN a.is_curator IS NULL THEN 0 ELSE a.is_curator END, \
                         COALESCE(a.trust_level, 0), ra.score_bp, COALESCE(ra.evidence_bp, 0), \
                         (SELECT group_concat(b.badge, ',') FROM live_badge_state b \
                           WHERE b.account_id = a.id AND b.held = 1) \
                  FROM accounts a LEFT JOIN reader_affinity ra ON ra.account_id = a.id \
                  WHERE a.deleted_at IS NULL";
    let postgres = "SELECT a.id::text, COALESCE(a.is_curator, false), \
                           COALESCE(a.trust_level, 0)::bigint, ra.score_bp, COALESCE(ra.evidence_bp, 0)::bigint, \
                           (SELECT string_agg(b.badge, ',') FROM live_badge_state b \
                             WHERE b.account_id = a.id::text AND b.held = true) \
                    FROM accounts a LEFT JOIN reader_affinity ra ON ra.account_id = a.id::text \
                    WHERE a.deleted_at IS NULL";
    let statement = db.sql(sqlite, postgres);
    let mut rows = Vec::new();
    match db.backend() {
        Backend::Sqlite => {
            let fetched = sqlx::query_as::<_, (String, i64, i64, Option<i64>, i64, Option<String>)>(&statement)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?;
            for (id, curator, trust, score_bp, evidence_bp, badges) in fetched {
                rows.push(ContributorRow {
                    id,
                    is_curator: curator != 0,
                    trust_level: u8::try_from(trust.clamp(0, 255)).unwrap_or(0),
                    affinity: score_bp.map(|bp| f64::from(i32::try_from(bp).unwrap_or(0)) / 10_000.0),
                    affinity_evidence: u32::try_from((evidence_bp / 1_000).max(0)).unwrap_or(0),
                    badges: split_badges(badges),
                });
            }
        }
        Backend::Postgres => {
            let fetched = sqlx::query_as::<_, (String, bool, i64, Option<i32>, i64, Option<String>)>(&statement)
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?;
            for (id, curator, trust, score_bp, evidence_bp, badges) in fetched {
                rows.push(ContributorRow {
                    id,
                    is_curator: curator,
                    trust_level: u8::try_from(trust.clamp(0, 255)).unwrap_or(0),
                    affinity: score_bp.map(|bp| f64::from(bp) / 10_000.0),
                    affinity_evidence: u32::try_from((evidence_bp / 1_000).max(0)).unwrap_or(0),
                    badges: split_badges(badges),
                });
            }
        }
    }
    Ok(rows)
}

/// Every act that can set the instance's taste, oldest included (the fold is what decides an age is too old).
///
/// `limit` bounds a first run on a large instance: the fold is O(rows), and the caller can raise it rather than discover
/// an hour-long request.
pub async fn evidence(db: &Database, limit: i64) -> Result<Vec<EvidenceRow>> {
    let sqlite = "SELECT r.account_id, 'rating', wt.node_id, r.stars / 5.0, \
                         julianday('now') - julianday(r.created_at) \
                  FROM rating r JOIN work_tags wt ON wt.work_id = r.work_id \
                  WHERE r.deleted_at IS NULL AND r.stars > 0 \
                  UNION ALL \
                  SELECT c.owner, 'collection', wt.node_id, 1.0, \
                         julianday('now') - julianday(ci.added_at) \
                  FROM collection_items ci JOIN collections c ON c.id = ci.collection_id \
                       JOIN work_tags wt ON wt.work_id = ci.work_id \
                  WHERE c.is_public = 1 \
                  UNION ALL \
                  SELECT sv.account_id, 'saved_search', sv.name, 1.0, \
                         julianday('now') - julianday(sv.created_at) \
                  FROM saved_views sv \
                  LIMIT ?";
    let postgres = "SELECT r.account_id::text, 'rating', wt.node_id, r.stars::float8 / 5.0, \
                           EXTRACT(EPOCH FROM (now() - r.created_at::timestamptz)) / 86400.0 \
                    FROM rating r JOIN work_tags wt ON wt.work_id = r.work_id \
                    WHERE r.deleted_at IS NULL AND r.stars > 0 \
                    UNION ALL \
                    SELECT c.owner, 'collection', wt.node_id, 1.0, \
                           EXTRACT(EPOCH FROM (now() - ci.added_at::timestamptz)) / 86400.0 \
                    FROM collection_items ci JOIN collections c ON c.id = ci.collection_id \
                         JOIN work_tags wt ON wt.work_id::text = ci.work_id \
                    WHERE c.is_public = 1 \
                    UNION ALL \
                    SELECT sv.account_id::text, 'saved_search', sv.name, 1.0, \
                           EXTRACT(EPOCH FROM (now() - sv.created_at::timestamptz)) / 86400.0 \
                    FROM saved_views sv \
                    LIMIT ?";
    let statement = db.sql(sqlite, postgres);
    let mut rows = Vec::new();
    match db.backend() {
        Backend::Sqlite => {
            let fetched = sqlx::query_as::<_, (String, String, String, f64, Option<f64>)>(&statement)
                .bind(limit)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?;
            for (contributor, verb, subject, strength, age) in fetched {
                rows.push(EvidenceRow {
                    contributor,
                    verb,
                    subject,
                    strength,
                    age_days: age.unwrap_or(0.0),
                });
            }
        }
        Backend::Postgres => {
            let fetched = sqlx::query_as::<_, (String, String, String, f64, Option<f64>)>(&statement)
                .bind(limit)
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?;
            for (contributor, verb, subject, strength, age) in fetched {
                rows.push(EvidenceRow {
                    contributor,
                    verb,
                    subject,
                    strength,
                    age_days: age.unwrap_or(0.0),
                });
            }
        }
    }
    Ok(rows)
}

/// `"attuned,curator"` → `["attuned", "curator"]`, dropping empty pieces.
fn split_badges(joined: Option<String>) -> Vec<String> {
    joined
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|piece| !piece.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::split_badges;

    #[test]
    fn a_badge_list_is_split_and_trimmed() {
        assert_eq!(split_badges(Some("attuned, curator".to_owned())), vec!["attuned", "curator"]);
    }

    #[test]
    fn no_badges_is_an_empty_list_not_one_empty_badge() {
        assert!(split_badges(None).is_empty());
        assert!(split_badges(Some(String::new())).is_empty());
    }
}
