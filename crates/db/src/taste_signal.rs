//! A reader's taste signal for a page of candidates, from what they have shelved (M64-09, spec §16.2).
//!
//! The gravity layer was mechanically complete and inert: `taste_signal` was 0.0 for every candidate, so the mixture
//! `(1-g)*base + g*taste` reduced to `base` and the pull could not move anything. A correct layer with no input is not a
//! feature, and inventing a number would be worse than the silence — so this reads a signal the schema already holds.
//!
//! **The signal is overlap with the reader's own shelf.** Take the canonical entities on the works they shelved, and score a
//! candidate by the share of *its* entities that appear among them:
//!
//! ```text
//! signal = |candidate entities ∩ shelf entities| / |candidate entities|
//! ```
//!
//! Share of the candidate, not of the shelf, deliberately: a reader who has shelved two hundred works has a mostly-
//! meaningless affinity set, and dividing by it would make every candidate look weakly aligned. Dividing by the candidate
//! asks the narrower and answerable question — "how much of this work do I recognise?".
//!
//! Three limits, each chosen rather than inherited:
//!
//! * **only entities that exist on both sides count.** A candidate with no tags gets 0.0, not 1.0: "we know nothing about
//!   it" is not "it is exactly your taste";
//! * **the shelf is the whole input**, because it is the one collection a reader assembled on purpose. Ratings would be a
//!   better signal and this schema does not keep them per reader (`work_ratings` holds an aggregate `(mu, phi, sigma)`, not a
//!   row per pseud), so using them here would mean inventing the association;
//! * **no history means no signal**, and the caller sees `None` rather than a map of zeros, so a surface can say it has
//!   nothing to go on instead of pretending a zero is a measurement.

use crate::{Backend, Database, Result};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// One entry per candidate the signal could be computed for.
///
/// A candidate absent from the map had no measured entities and therefore no signal; a candidate present with `0.0` was
/// measured and shares nothing with the shelf. The two are different facts, which is why this is not one map of `f64`.
pub type Signals = HashMap<Uuid, f64>;

/// Taste signals for `work_ids`, from the works `account_id` has shelved.
///
/// Returns `None` when the account has no shelf with anything on it — there is nothing to compute against, and a surface that
/// cannot tell "no overlap" from "no history" would report a measurement it never made.
pub async fn for_account(
    db: &Database,
    account_id: &str,
    work_ids: &[Uuid],
) -> Result<Option<Signals>> {
    if work_ids.is_empty() {
        return Ok(None);
    }

    // The reader's affinity set: canonical entities on the works they shelved.
    let shelf_sql = match db.backend() {
        Backend::Sqlite => {
            "SELECT DISTINCT wt.node_id FROM work_tags wt \
             WHERE wt.work_id IN (SELECT li.work_id FROM shelf_items si \
                                    JOIN shelves s ON s.id = si.shelf_id \
                                    JOIN library_items li ON li.id = si.library_item_id \
                                   WHERE s.account_id = ?1)"
        }
        Backend::Postgres => {
            "SELECT DISTINCT wt.node_id FROM work_tags wt \
             WHERE wt.work_id IN (SELECT li.work_id FROM shelf_items si \
                                    JOIN shelves s ON s.id = si.shelf_id \
                                    JOIN library_items li ON li.id = si.library_item_id \
                                   WHERE s.account_id = $1::uuid)"
        }
    };
    // `$1::uuid`: `shelves.account_id` is UUID on PostgreSQL and the parameter arrives as `&str`, so an uncast bind is
    // `operator does not exist: uuid = text`. The callers swallow the error (`_ => ranked`), which is how this survived:
    // on the deployed engine every reader with a shelf got a failed query and a page that ran silently unpulled — the exact
    // M64-09 defect, still live. Found by running this module's PG arm, which nothing had done since the fix landed.
    let shelf: Vec<String> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(shelf_sql)
                .bind(account_id)
                .fetch_all(db.sqlite_pool().expect("sqlite pool for sqlite backend"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(shelf_sql)
                .bind(account_id)
                .fetch_all(
                    db.postgres_pool()
                        .expect("postgres pool for postgres backend"),
                )
                .await?
        }
    };
    if shelf.is_empty() {
        return Ok(None);
    }
    let shelf: HashSet<String> = shelf.into_iter().collect();

    // The candidates' own entities, in one query for the page rather than one per row.
    let mut entity_rows: Vec<(String, String)> = Vec::new();
    for chunk in work_ids.chunks(200) {
        let placeholders: Vec<String> = match db.backend() {
            Backend::Sqlite => (1..=chunk.len()).map(|index| format!("?{index}")).collect(),
            Backend::Postgres => (1..=chunk.len()).map(|index| format!("${index}")).collect(),
        };
        let sql = format!(
            "SELECT CAST(work_id AS TEXT), node_id FROM work_tags WHERE work_id IN ({})",
            placeholders.join(", ")
        );
        match db.backend() {
            Backend::Sqlite => {
                let mut query = sqlx::query_as::<_, (String, String)>(&sql);
                for id in chunk {
                    query = query.bind(id.to_string());
                }
                entity_rows.extend(
                    query
                        .fetch_all(db.sqlite_pool().expect("sqlite pool for sqlite backend"))
                        .await?,
                );
            }
            Backend::Postgres => {
                let mut query = sqlx::query_as::<_, (String, String)>(&sql);
                for id in chunk {
                    query = query.bind(*id);
                }
                entity_rows.extend(
                    query
                        .fetch_all(
                            db.postgres_pool()
                                .expect("postgres pool for postgres backend"),
                        )
                        .await?,
                );
            }
        }
    }

    let mut per_work: HashMap<String, (usize, usize)> = HashMap::new();
    for (work_id, node_id) in entity_rows {
        let entry = per_work.entry(work_id).or_insert((0, 0));
        entry.1 += 1;
        if shelf.contains(&node_id) {
            entry.0 += 1;
        }
    }

    let mut signals = Signals::new();
    for work_id in work_ids {
        // Absent means the work carries no entities we know about: no row, no signal. Present with a zero denominator cannot
        // happen (the map only holds works that had rows), but the guard keeps the division honest rather than clever.
        if let Some((matched, total)) = per_work.get(&work_id.to_string()) {
            if *total > 0 {
                signals.insert(*work_id, *matched as f64 / *total as f64);
            }
        }
    }
    Ok(Some(signals))
}
