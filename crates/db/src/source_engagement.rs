//! Engagement as each archive reports it (M63-37).
//!
//! `library_items.likes` is one normalised number. It is the right shape for *ranking* and the wrong shape for a *rate*: AO3 publishes
//! hits and kudos, the XenForo boards publish replies and threadmark likes, FFNet publishes favourites and follows. Dividing one
//! archive's normalised count by another's produces a number that looks like a rate and is not one.
//!
//! So each metric is stored under the name its source published it, and a rate is computed only from two metrics **of the same
//! archive** — which is what `likes_per_view` does, and why it returns `None` rather than `0.0` when the denominator is zero or absent.
//! A zero rate and an unmeasurable one are different facts, and a ranking that treats them the same will happily promote a story
//! nobody measured.
//!
//! The normalised `library_items.likes` column stays: it is what the catalogue displays and what `crosspost_union` sums, and replacing it
//! here would mean two migrations' worth of readers changing at once for no gain. This table is the record of *what each archive said*,
//! which is the thing the card asks for.

use crate::{Backend, Database, Result};
use serde::Serialize;

/// A metric an archive reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Metric {
    /// Endorsements by whatever the archive calls them (kudos, favourites, likes).
    Likes,
    /// Page hits, where the archive publishes them (AO3 `hits`).
    Hits,
    /// Views, where the archive distinguishes them from hits.
    Views,
    /// Reply count (the boards' thread replies).
    Replies,
    /// Bookmarks or subscriptions to a work.
    Bookmarks,
    /// AO3's own name for an endorsement.
    Kudos,
    /// FFNet's follow count.
    Follows,
    /// FFNet's favourite count.
    Favourites,
}

impl Metric {
    /// The stored name — the archive's own word where there is one.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Likes => "likes",
            Self::Hits => "hits",
            Self::Views => "views",
            Self::Replies => "replies",
            Self::Bookmarks => "bookmarks",
            Self::Kudos => "kudos",
            Self::Follows => "follows",
            Self::Favourites => "favourites",
        }
    }

    /// Every metric this build knows, which is the closed set the table's `CHECK` enforces.
    pub fn all() -> [Self; 8] {
        [
            Self::Likes,
            Self::Hits,
            Self::Views,
            Self::Replies,
            Self::Bookmarks,
            Self::Kudos,
            Self::Follows,
            Self::Favourites,
        ]
    }

    /// Parse a stored name. `None` for a word this build does not know, so a row written by a newer importer is reported rather than
    /// silently folded into `likes`.
    pub fn parse(value: &str) -> Option<Self> {
        Self::all()
            .into_iter()
            .find(|metric| metric.as_str() == value)
    }
}

/// One reading: what an archive said, and when it said it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Reading {
    /// The metric's name.
    pub metric: String,
    /// The count.
    pub value: i64,
    /// When the source reported it, RFC 3339.
    pub reported_at: String,
}

/// Record (or correct) one metric for one imported item.
///
/// Upsert, not append: an archive's counter moves, and a history of counters is a different table with a different purpose. What a rate
/// needs is the pair that was published together, so the newest reading replaces the older one — `reported_at` travels with it so a
/// caller can tell a fresh number from a stale one.
pub async fn record(
    db: &Database,
    library_item_id: &str,
    metric: Metric,
    value: i64,
    reported_at: &str,
) -> Result<()> {
    if value < 0 {
        anyhow::bail!("an engagement count cannot be negative: {metric:?} = {value}");
    }
    let sql = db.sql(
        "INSERT INTO library_item_engagement (library_item_id, metric, value, reported_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (library_item_id, metric) DO UPDATE SET
             value = excluded.value,
             reported_at = excluded.reported_at",
        "INSERT INTO library_item_engagement (library_item_id, metric, value, reported_at)
         VALUES ($1::uuid, $2, $3, $4)
         ON CONFLICT (library_item_id, metric) DO UPDATE SET
             value = excluded.value,
             reported_at = excluded.reported_at",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(library_item_id)
                .bind(metric.as_str())
                .bind(value)
                .bind(reported_at)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(library_item_id)
                .bind(metric.as_str())
                .bind(value)
                .bind(reported_at)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }
    Ok(())
}

/// Everything one archive said about one item, in the vocabulary's own order.
pub async fn readings(db: &Database, library_item_id: &str) -> Result<Vec<Reading>> {
    let sql = db.sql(
        "SELECT metric, value, reported_at FROM library_item_engagement
          WHERE library_item_id = ?1 ORDER BY metric",
        "SELECT metric, value::bigint, reported_at FROM library_item_engagement
          WHERE library_item_id = $1::uuid ORDER BY metric",
    );
    let rows: Vec<(String, i64, String)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(library_item_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(library_item_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows
        .into_iter()
        .map(|(metric, value, reported_at)| Reading {
            metric,
            value,
            reported_at,
        })
        .collect())
}

/// Endorsements per view, **from one archive's own pair of counts**.
///
/// `None` in three cases, all different from a rate of zero:
///
/// * either metric is absent — the archive does not publish it, so there is nothing to divide;
/// * `views` is zero — a rate over no views is undefined, not zero;
/// * either metric is negative — refused at the write, but a row from another writer is still checked here rather than trusted.
///
/// The caller gets the ratio only when both halves are real, which is the entire point of storing per-source metrics.
pub async fn likes_per_view(
    db: &Database,
    library_item_id: &str,
    like_metric: Metric,
    view_metric: Metric,
) -> Result<Option<f64>> {
    let mut likes = None;
    let mut views = None;
    for reading in readings(db, library_item_id).await? {
        if reading.metric == like_metric.as_str() {
            likes = Some(reading.value);
        }
        if reading.metric == view_metric.as_str() {
            views = Some(reading.value);
        }
    }
    let (Some(likes), Some(views)) = (likes, views) else {
        return Ok(None);
    };
    if views <= 0 || likes < 0 {
        return Ok(None);
    }
    Ok(Some(likes as f64 / views as f64))
}
