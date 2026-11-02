//! §9.4 — storage for the labelled one-click reaction bar (M47-12).
//!
//! One row per reader per label per work. The uniqueness is the schema's (`quick_reaction_once`),
//! not a rule this module remembers, so every read below is a plain count rather than a
//! `COUNT(DISTINCT …)` a later query could forget.

use anyhow::Result;
use uuid::Uuid;

use crate::{Backend, Database};

/// Whether a work's author allows feedback on it.
///
/// §9.4 says reactions are "subject to the author's feedback preferences". The schema carries one
/// such preference — `comments_enabled`, at the work level with an account-level fallback — so that
/// is what this reads: a reaction is feedback, and an author who turned feedback off on a work does
/// not get a bar they did not ask for.
///
/// Resolved in ONE statement, because the fallback chain is the whole feature: work override, then
/// the owner's account default, then enabled. Three round trips would let a concurrent preference
/// write split the answer across them.
pub async fn enabled_for(db: &Database, work_id: &str) -> Result<bool> {
    let sql = db.sql(
        "SELECT COALESCE(wfp.comments_enabled, fp.comments_enabled, 1)
           FROM works w
           JOIN pseuds p ON p.id = w.owner_pseud_id
           LEFT JOIN work_feedback_preferences wfp ON wfp.work_id = w.id
           LEFT JOIN feedback_preferences fp ON fp.account_id = p.account_id
          WHERE w.id = ?",
        "SELECT COALESCE(wfp.comments_enabled::int, fp.comments_enabled::int, 1)::bigint
           FROM works w
           JOIN pseuds p ON p.id::text = w.owner_pseud_id::text
           LEFT JOIN work_feedback_preferences wfp ON wfp.work_id = w.id
           LEFT JOIN feedback_preferences fp ON fp.account_id = p.account_id
          WHERE w.id = ?::uuid",
    );
    let value: Option<i64> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&sql)
                .bind(work_id)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&sql)
                .bind(work_id)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    // A work that does not exist answers "not enabled": the caller is about to 404 on the work
    // anyway, and defaulting to enabled would offer a bar on nothing.
    Ok(value.unwrap_or(0) != 0)
}

/// Record one reaction. Idempotent: a double-tap is one reaction, not an error.
pub async fn add(db: &Database, work_id: &str, pseud_id: &str, label: &str, now: &str) -> Result<()> {
    let sql = db.sql(
        "INSERT INTO quick_reaction (id, work_id, pseud_id, label, created_at)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT (work_id, pseud_id, label) DO NOTHING",
        "INSERT INTO quick_reaction (id, work_id, pseud_id, label, created_at)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT (work_id, pseud_id, label) DO NOTHING",
    );
    let id = Uuid::new_v4().to_string();
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(&id)
                .bind(work_id)
                .bind(pseud_id)
                .bind(label)
                .bind(now)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(&id)
                .bind(work_id)
                .bind(pseud_id)
                .bind(label)
                .bind(now)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }
    Ok(())
}

/// Withdraw one reaction. Returns whether there was one to withdraw, so a caller can tell
/// "removed" from "you had not reacted" without a second read.
pub async fn remove(db: &Database, work_id: &str, pseud_id: &str, label: &str) -> Result<bool> {
    let sql = db.sql(
        "DELETE FROM quick_reaction WHERE work_id = ? AND pseud_id = ? AND label = ?",
        "DELETE FROM quick_reaction WHERE work_id = ? AND pseud_id = ? AND label = ?",
    );
    let affected = match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(work_id)
                .bind(pseud_id)
                .bind(label)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?
                .rows_affected()
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(work_id)
                .bind(pseud_id)
                .bind(label)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?
                .rows_affected()
        }
    };
    Ok(affected > 0)
}

/// The public tally: `(label, count)` for every label with at least one reaction, most-reacted
/// first and then alphabetically so the order is stable between reads.
///
/// Labels with no votes are omitted rather than returned as zero: the vocabulary is the frontend's
/// own copy, so a zero adds nothing and would make the payload grow with the vocabulary.
pub async fn tally(db: &Database, work_id: &str) -> Result<Vec<(String, i64)>> {
    let sql = db.sql(
        "SELECT label, COUNT(*) AS reactions
           FROM quick_reaction WHERE work_id = ?
          GROUP BY label ORDER BY reactions DESC, label ASC",
        "SELECT label, COUNT(*)::bigint AS reactions
           FROM quick_reaction WHERE work_id = ?
          GROUP BY label ORDER BY reactions DESC, label ASC",
    );
    let rows: Vec<(String, i64)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(work_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(work_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows)
}

/// The labels this reader has reacted with, in vocabulary order.
pub async fn mine(db: &Database, work_id: &str, pseud_id: &str) -> Result<Vec<String>> {
    let sql = db.sql(
        "SELECT label FROM quick_reaction WHERE work_id = ? AND pseud_id = ?",
        "SELECT label FROM quick_reaction WHERE work_id = ? AND pseud_id = ?",
    );
    let rows: Vec<(String,)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(work_id)
                .bind(pseud_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(work_id)
                .bind(pseud_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    let held: Vec<String> = rows.into_iter().map(|(label,)| label).collect();
    Ok(darkhold_domain::quick_reactions::slugs()
        .into_iter()
        .filter(|slug| held.iter().any(|label| label == slug))
        .map(str::to_owned)
        .collect())
}
