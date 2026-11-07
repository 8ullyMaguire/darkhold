//! How much of the tagged catalogue anybody has engaged with (M64-06).
//!
//! A tag algorithm is not short of tags or short of engagement; the failure it actually has is that the two sets are **disjoint**.
//! Measured on the live instance 2026-11-01: `work_tags` joins 0 of the 1,247 rows that carry engagement — every tagged work is an
//! import nobody has read. An instance in that state has plenty of rows in both tables, so a "table is empty" refusal never fires
//! and the surface reports that it is serving `tag_jaccard` while returning nothing that reads as "nothing is similar".
//!
//! This module answers the question the refusal needs: **how many works are both tagged and engaged?** Engagement is the tree's own
//! definition (`work_metric_aggregates`: views, complete reads, reactions, kudos, bookmarks, collection adds, reviews), because a
//! second definition here would be a second answer to "has anybody read this".
//!
//! The numbers are reported, not just the verdict: an operator who sees `0 of 7,626 tagged works carry engagement` knows the fix is a
//! backfill, and one who sees `7,626 of 7,626` knows a zero result is real.

use crate::{Backend, Database, Result};

/// The three counts the refusal needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TagEngagement {
    /// Works carrying at least one tag.
    pub tagged: i64,
    /// Works with any engagement at all.
    pub engaged: i64,
    /// Works that are both — the only rows a tag algorithm can rank from.
    pub tagged_and_engaged: i64,
}

impl TagEngagement {
    /// Whether a tag algorithm has anything to rank. Zero tagged works and zero overlap are different failures with different
    /// fixes, so the caller distinguishes them rather than treating "no" as one thing.
    pub fn usable(&self) -> bool {
        self.tagged_and_engaged > 0
    }
}

/// Measure the overlap between the tagged catalogue and the engaged one.
pub async fn tag_engagement(db: &Database) -> Result<TagEngagement> {
    // One statement per count and both engines spelled out. `work_metric_aggregates` is one row per work, maintained by the
    // application layer, so the "engaged" side is a single indexed scan rather than a union over every event table.
    let engaged_sum = "(m.views + m.complete_reads + m.reactions + m.kudos + m.bookmarks + m.collection_adds + m.reviews)";

    let tagged_sql = "SELECT COUNT(DISTINCT work_id) FROM work_tags";
    let engaged_sql =
        format!("SELECT COUNT(*) FROM work_metric_aggregates m WHERE {engaged_sum} > 0");
    let overlap_sql = format!(
        "SELECT COUNT(*) FROM (SELECT DISTINCT work_id FROM work_tags) t
           JOIN work_metric_aggregates m ON m.work_id = t.work_id
          WHERE {engaged_sum} > 0"
    );

    let tagged: i64 = scalar(db, tagged_sql).await?;
    let engaged: i64 = scalar(db, &engaged_sql).await?;
    let tagged_and_engaged: i64 = scalar(db, &overlap_sql).await?;
    Ok(TagEngagement {
        tagged,
        engaged,
        tagged_and_engaged,
    })
}

async fn scalar(db: &Database, sql: &str) -> Result<i64> {
    // No binds: these are whole-table counts, and adding a parameter would mean a caller could narrow them and get a refusal that
    // describes a subset of the instance.
    Ok(match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(sql)
                .fetch_one(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(sql)
                .fetch_one(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    })
}
