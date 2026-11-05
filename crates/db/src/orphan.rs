//! M70-01 — orphaning a work: the row survives, the author link does not.
//!
//! `accounts → pseuds → works` is `NOT NULL … ON DELETE CASCADE` at every link, so deleting an account deletes the
//! works. Orphaning is the other outcome, and it is reached by **rekeying** rather than by widening a column (see
//! ADR 0004): the work is moved onto a fresh pseud owned by the instance's system account, and the original pseud
//! is then referenced by nothing — free to be deleted with its account.
//!
//! # Why a pseud per orphaned work, not one shared pseud
//!
//! A single `'orphaned'` pseud would re-link every orphaned work to one anonymous author. Two readers who
//! bookmarked two different works would see the same name and could join the two — the opposite of what the author
//! asked for. Each orphaning gets its own pseud, and the handle is derived from the work id so uniqueness holds by
//! construction rather than by a counter somebody has to remember to increment.
//!
//! # What stays behind
//!
//! Everything that references the *work*: chapters, comments, kudos, collections, bookmarks, tags. None of those
//! point at the pseud, so none of them move — which is why orphaning is a two-statement operation and not a
//! migration.

use crate::{Backend, Database, SYSTEM_ACCOUNT};

/// Orphan a work.
///
/// Returns the frozen author line on success, and `None` when the work is already orphaned (its pseud already
/// belongs to the system account) — orphaning twice is not an error, it is a no-op, because there is no owner left
/// to detach and appending the suffix again would imply one.
///
/// Returns `Ok(None)` for a missing work too: from the caller's side "there is nothing here to detach" is the same
/// answer, and a route that distinguishes them leaks whether a work id exists.
pub async fn orphan_work(db: &Database, work_id: &str) -> Result<Option<String>, sqlx::Error> {
    let select = db.sql(
        "SELECT p.display_name, p.account_id \
           FROM works w JOIN pseuds p ON p.id = w.owner_pseud_id \
          WHERE w.id = ?",
        "SELECT p.display_name, p.account_id::text \
           FROM works w JOIN pseuds p ON p.id = w.owner_pseud_id \
          WHERE w.id = $1::uuid",
    );

    // One transaction per engine: a work must never end up pointing at a pseud that was not created, and a pseud
    // must never be created for a work that then keeps its old owner.
    match db.backend() {
        Backend::Sqlite => {
            let pool = db.sqlite_pool().expect("sqlite handle");
            let mut tx = pool.begin().await?;
            let found: Option<(String, String)> = sqlx::query_as(&select)
                .bind(work_id)
                .fetch_optional(&mut *tx)
                .await?;
            let Some((display_name, owner)) = found else {
                return Ok(None);
            };
            if owner == SYSTEM_ACCOUNT.to_string() {
                return Ok(None);
            }
            let label = darkhold_domain::orphan::freeze(&display_name, true, false)
                .map_err(|reason| sqlx::Error::Protocol(format!("cannot orphan: {reason:?}")))?;
            let handle = detached_handle(work_id);
            sqlx::query(
                "INSERT INTO pseuds (id, account_id, handle, display_name, bio, discoverability, created_at, \
                 updated_at, version) \
                 VALUES (?, ?, ?, ?, NULL, 'listed', strftime('%Y-%m-%dT%H:%M:%SZ','now'), \
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1)",
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(SYSTEM_ACCOUNT.to_string())
            .bind(&handle)
            .bind(label.as_str())
            .execute(&mut *tx)
            .await?;
            sqlx::query("UPDATE works SET owner_pseud_id = (SELECT id FROM pseuds WHERE handle = ?) WHERE id = ?")
                .bind(&handle)
                .bind(work_id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            Ok(Some(label.as_str().to_string()))
        }
        Backend::Postgres => {
            let pool = db.postgres_pool().expect("postgres handle");
            let mut tx = pool.begin().await?;
            let found: Option<(String, String)> = sqlx::query_as(&select)
                .bind(work_id)
                .fetch_optional(&mut *tx)
                .await?;
            let Some((display_name, owner)) = found else {
                return Ok(None);
            };
            if owner == SYSTEM_ACCOUNT.to_string() {
                return Ok(None);
            }
            let label = darkhold_domain::orphan::freeze(&display_name, true, false)
                .map_err(|reason| sqlx::Error::Protocol(format!("cannot orphan: {reason:?}")))?;
            let handle = detached_handle(work_id);
            sqlx::query(
                "INSERT INTO pseuds (id, account_id, handle, display_name, bio, discoverability, created_at, \
                 updated_at, version) \
                 VALUES ($1::uuid, $2::uuid, $3, $4, NULL, 'listed', \
                 to_char(now() at time zone 'utc','YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), \
                 to_char(now() at time zone 'utc','YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), 1)",
            )
            .bind(uuid::Uuid::new_v4())
            .bind(SYSTEM_ACCOUNT)
            .bind(&handle)
            .bind(label.as_str())
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                "UPDATE works SET owner_pseud_id = (SELECT id FROM pseuds WHERE handle = $1) WHERE id = $2::uuid",
            )
            .bind(&handle)
            .bind(work_id)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            Ok(Some(label.as_str().to_string()))
        }
    }
}

/// The handle of a detached pseud, derived from the work id so two orphans of two works cannot collide under the
/// global `pseuds_handle_normalized` index.
fn detached_handle(work_id: &str) -> String {
    format!("orphaned-{}", work_id.to_lowercase())
}
