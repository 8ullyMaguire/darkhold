//! Which rated works a reader may actually be shown.
//!
//! The numbers come from `polaris-taste` (`crates/app/src/taste.rs`); everything that decides *eligibility*
//! stays here, because it is about darkhold's own content and darkhold's own reader:
//!
//! 1. **`min_matches`** — a rating with a deviation of 228 out of an initial 350 is not evidence.
//! 2. **`lifecycle = 'published' AND visibility = 'public'`** — a draft is not a candidate for anything.
//! 3. **the reader's content filters**, through `content_filter_sql`, exactly as every other engine applies
//!    them. This is the one place a rating could surface a work the reader asked never to see.
//! 4. **the reader's own ballots** — a work they have just compared is a work they have just judged.
//!
//! The candidates are bounded by the caller (a feed asks for a shelf, not a leaderboard), so this filters a
//! list rather than querying a table: the ordering is the service's, and re-ordering in SQL would quietly
//! make this a second ladder.

use crate::{Backend, Database};

/// One rated work, as the service reported it.
#[derive(Debug, Clone, PartialEq)]
pub struct LadderWork {
    pub work_id: String,
    pub rating: f64,
    pub deviation: f64,
    pub matches: i64,
}

/// The eligible subset of `candidates`, in the order they arrived, at most `limit` of them.
///
/// An empty candidate list is not an error: a fresh instance has no ratings and its feed is still a feed.
pub async fn eligible_top(
    db: &Database,
    candidates: &[LadderWork],
    account_id: Option<&str>,
    viewer_pseud: Option<uuid::Uuid>,
    limit: i64,
    min_matches: i64,
) -> anyhow::Result<Vec<LadderWork>> {
    if limit <= 0 {
        return Err(anyhow::anyhow!("limit must be positive"));
    }
    let eligible_pool: Vec<&LadderWork> = candidates
        .iter()
        .filter(|candidate| candidate.matches >= min_matches)
        .collect();
    if eligible_pool.is_empty() {
        return Ok(Vec::new());
    }

    // The IN list is built from the ids actually asked about, so the query can never widen itself.
    let ids: Vec<String> = eligible_pool.iter().map(|c| c.work_id.clone()).collect();
    let marks = vec!["?"; ids.len()].join(",");
    let rules = crate::search::content_filter_sql::for_pseud(db, viewer_pseud).await?;
    let filter = crate::search::content_filter_sql::exclusion_for(&rules, "w.id");

    // Bind order is positional: the filter's own binds, then the ids, then the account.
    let sqlite = format!(
        "SELECT CAST(w.id AS TEXT)
         FROM works w
         WHERE w.lifecycle = 'published' AND w.visibility = 'public'
           {filter_clause}
           AND CAST(w.id AS TEXT) IN ({marks})
           AND NOT EXISTS (
             SELECT 1 FROM arena_ballots b
             WHERE b.account_id = ?
               AND (b.best_work_id = w.id OR b.worst_work_id = w.id)
           )",
        filter_clause = filter.clause()
    );
    // `arena_ballots.account_id` is UUID on PostgreSQL; a text bind against it is a type error, which is the
    // one place the two dialects disagree here.
    let postgres = sqlite.replace("b.account_id = ?", "b.account_id = ?::uuid");

    let rows: Vec<(String,)> = match db.backend() {
        Backend::Sqlite => {
            let mut query = sqlx::query_as(&sqlite);
            for bind in &filter.binds {
                query = query.bind(bind);
            }
            for id in &ids {
                query = query.bind(id);
            }
            query
                .bind(account_id)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            let sql = crate::sql_owned(db, sqlite, postgres);
            let mut query = sqlx::query_as(&sql);
            for bind in &filter.binds {
                query = query.bind(bind);
            }
            for id in &ids {
                query = query.bind(id);
            }
            query
                .bind(account_id)
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?
        }
    };

    let allowed: std::collections::HashSet<&str> = rows.iter().map(|(id,)| id.as_str()).collect();
    Ok(eligible_pool
        .into_iter()
        .filter(|candidate| allowed.contains(candidate.work_id.as_str()))
        .take(limit as usize)
        .cloned()
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(id: &str, matches: i64) -> LadderWork {
        LadderWork {
            work_id: id.to_string(),
            rating: 1600.0,
            deviation: 200.0,
            matches,
        }
    }

    #[test]
    fn the_match_floor_is_applied_before_any_query() {
        // One ballot gives the middles two comparisons, so `matches >= 3` is the evidence floor. A
        // candidate below it must not even reach the eligibility query.
        let candidates = vec![candidate("a", 1), candidate("b", 5)];
        let kept: Vec<&LadderWork> = candidates.iter().filter(|c| c.matches >= 3).collect();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].work_id, "b");
    }

    #[test]
    fn the_sql_is_bounded_by_the_ids_it_was_given() {
        let ids = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let marks = vec!["?"; ids.len()].join(",");
        assert_eq!(marks, "?,?,?");
        assert!(!marks.contains('*'));
    }
}
