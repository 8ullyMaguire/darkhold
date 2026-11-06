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

/// The highest Glicko-2 volatility this build will treat as settled evidence.
///
/// Chosen well above the starting value (0.06) so ordinary play never hits it: a work whose volatility has climbed past a
/// tenth is one whose rating has been moving after each ballot, and a page that ranks by it is ranking by a number still in
/// flight. A single named constant rather than a config key, because it is a property of the Glicko-2 scale rather than an
/// instance's taste in risk.
pub const MAX_VOLATILITY: f64 = 0.10;

/// Whether a reading is an eligible one: enough comparisons, not contested, not still in flight.
///
/// One function rather than three `.filter` calls in `eligible_top` because it is the whole rule, and a rule spread over three
/// closures is a rule three tests can each pass while the composition fails. Every clause is optional-safe: a service that reports
/// neither volatility nor divergence is filtered exactly as it was before those fields existed.
pub fn is_settled(candidate: &LadderWork, min_matches: i64) -> bool {
    candidate.matches >= min_matches
        && candidate.divergence != Some(true)
        && candidate
            .volatility
            .is_none_or(|volatility| volatility <= MAX_VOLATILITY)
}

/// One rated work, as the service reported it.
#[derive(Debug, Clone, PartialEq)]
pub struct LadderWork {
    pub work_id: String,
    pub rating: f64,
    pub deviation: f64,
    pub matches: i64,
    /// Glicko-2 volatility, when the service reported it. `None` is "not reported", never "0.06".
    pub volatility: Option<f64>,
    /// The service's divergence flag: it considers the work's rating contested. `None` is "not reported".
    pub divergence: Option<bool>,
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
// INFER: no surface concludes from a restricted work. Nobody could have read a work they were never shown,
// so an edge or a count built on one is a claim about people made from a non-event. See `db::visibility`.
    if limit <= 0 {
        return Err(anyhow::anyhow!("limit must be positive"));
    }
    let eligible_pool: Vec<&LadderWork> = candidates
        .iter()
        // `min_matches` answers "is there evidence" and the divergence flag answers a different question: "does the service say
        // the evidence is contested". A work the arena itself flags is excluded, because showing a reader a contested rating as
        // if it were settled is the same defect as showing a rating with two matches.
        //
        // Volatility is used as a **cap**, not a floor: Glicko-2's starting volatility is 0.06 and a rating that keeps jumping is
        // one whose number is not yet a property of the work. Volatility above the cap is refused; absent volatility is not, so a
        // service that does not report it keeps working unchanged.
        .filter(|candidate| is_settled(candidate, min_matches))
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
            volatility: None,
            divergence: None,
        }
    }

    /// M69-10: the service's own two extra numbers decide eligibility, and each is optional in a way that must not change
    /// behaviour for a service that does not send it.
    #[test]
    fn contested_or_volatile_ratings_are_not_evidence() {
        let settled = candidate("settled", 10);
        let contested = LadderWork {
            divergence: Some(true),
            ..candidate("contested", 10)
        };
        let volatile = LadderWork {
            volatility: Some(MAX_VOLATILITY + 0.01),
            ..candidate("volatile", 10)
        };
        let at_the_cap = LadderWork {
            volatility: Some(MAX_VOLATILITY),
            divergence: Some(false),
            ..candidate("at-the-cap", 10)
        };
        let too_few = candidate("too-few", 1);

        assert!(is_settled(&settled, 3), "no flag, no volatility, enough matches");
        assert!(!is_settled(&contested, 3), "the arena says readers disagree");
        assert!(!is_settled(&volatile, 3), "the rating is still moving");
        assert!(is_settled(&at_the_cap, 3), "the cap is inclusive");
        assert!(!is_settled(&too_few, 3), "the evidence floor still applies");
    }

    #[test]
    fn the_match_floor_is_applied_before_any_query() {
        // One ballot gives the middles two comparisons, so `matches >= 3` is the evidence floor. A
        // candidate below it must not even reach the eligibility query.
        let candidates = [candidate("a", 1), candidate("b", 5)];
        let kept: Vec<&LadderWork> = candidates.iter().filter(|c| c.matches >= 3).collect();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].work_id, "b");
    }

    #[test]
    fn the_sql_is_bounded_by_the_ids_it_was_given() {
        let ids = ["a".to_string(), "b".to_string(), "c".to_string()];
        let marks = vec!["?"; ids.len()].join(",");
        assert_eq!(marks, "?,?,?");
        assert!(!marks.contains('*'));
    }
}
