//! M69-07 — the evacuation store: announce, place, receive.
//!
//! The plan is `darkhold_domain::evacuation::plan`'s; this records it and, more importantly, records the *receipt*, which is
//! the half that outlives the shutdown. An evacuation nobody wrote down is indistinguishable from a database that lost its
//! rows, and the peer that took the work is the only copy left.
//!
//! # Confirming an acceptance is where the fork is refused
//!
//! The plan cannot place one work twice — `UNIQUE (notice_id, work_id)` — but a recipient can still tell an operator it took
//! something it was never offered, and a second recipient can say the same. So `settle` refuses an `accepted` state for a work
//! that already has an accepted row in this notice, and the refusal names the court it is already in. Refusing at the write is
//! the only place the rule bites: a receipt assembled afterwards and checked for forks is a receipt that already exists.

use anyhow::Result;
use sqlx::Row as _;

use darkhold_domain::evacuation::{Acceptance, Plan, Receipt, Unplaced};

use crate::{Backend, Database};

/// The open (or last) evacuation announcement.
#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    /// Its id.
    pub id: String,
    /// Why the instance is going away.
    pub reason: String,
    /// Who announced it.
    pub announced_by: String,
    /// When.
    pub announced_at: String,
    /// `announced`, `withdrawn` or `completed`.
    pub state: String,
    /// A free-form note.
    pub note: String,
    /// When it was completed.
    pub completed_at: Option<String>,
}

/// One work's placement in one notice.
#[derive(Debug, Clone, PartialEq)]
pub struct Assignment {
    /// Its id.
    pub id: String,
    /// The notice it belongs to.
    pub notice_id: String,
    /// The work.
    pub work_id: String,
    /// The work's title.
    pub title: String,
    /// The recipient.
    pub recipient_instance: String,
    /// Taste similarity when the plan was made, in basis points.
    pub similarity_bp: i64,
    /// `offered`, `accepted` or `declined`.
    pub state: String,
    /// When it was offered.
    pub offered_at: String,
    /// When the recipient settled it.
    pub settled_at: Option<String>,
    /// What the operator wrote down about the settlement.
    pub settle_note: Option<String>,
}

/// Announce a shutdown and record the plan it produced.
///
/// Refuses when a notice is already open: a second plan for one shutdown is the fork one level up from a work forking, and
/// merging two plans is a decision an operator makes (by withdrawing one), not one this function invents.
pub async fn announce(
    db: &Database,
    reason: &str,
    announced_by: &str,
    note: &str,
    plan: &Plan,
    now: &str,
) -> Result<(Notice, Vec<Assignment>)> {
    if reason.trim().is_empty() {
        return Err(anyhow::anyhow!(
            "an evacuation needs a reason: the receipt is read by people who were not here"
        ));
    }
    if let Some(open) = open_notice(db).await? {
        return Err(anyhow::anyhow!(
            "an evacuation is already announced ({}); withdraw it before announcing another",
            open.id
        ));
    }

    let id = uuid::Uuid::new_v4().to_string();
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO evacuation_notices (id, reason, announced_by, announced_at, state, note)
                 VALUES (?, ?, ?, ?, 'announced', ?)",
            )
            .bind(&id)
            .bind(reason.trim())
            .bind(announced_by)
            .bind(now)
            .bind(note)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO evacuation_notices (id, reason, announced_by, announced_at, state, note)
                 VALUES ($1, $2, $3, $4, 'announced', $5)",
            )
            .bind(&id)
            .bind(reason.trim())
            .bind(announced_by)
            .bind(now)
            .bind(note)
            .execute(db.postgres_pool().expect("postgres"))
            .await?;
        }
    }

    for placement in &plan.placements {
        let similarity_bp = (placement.similarity * 10_000.0).round().clamp(0.0, 10_000.0) as i64;
        let assignment_id = uuid::Uuid::new_v4().to_string();
        match db.backend() {
            Backend::Sqlite => {
                sqlx::query(
                    "INSERT INTO evacuation_assignments
                        (id, notice_id, work_id, title, recipient_instance, similarity_bp, state, offered_at)
                     VALUES (?, ?, ?, ?, ?, ?, 'offered', ?)",
                )
                .bind(&assignment_id)
                .bind(&id)
                .bind(&placement.work_id)
                .bind(&placement.title)
                .bind(&placement.recipient)
                .bind(similarity_bp)
                .bind(now)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await?;
            }
            Backend::Postgres => {
                sqlx::query(
                    "INSERT INTO evacuation_assignments
                        (id, notice_id, work_id, title, recipient_instance, similarity_bp, state, offered_at)
                     VALUES ($1, $2, $3, $4, $5, $6::bigint, 'offered', $7)",
                )
                .bind(&assignment_id)
                .bind(&id)
                .bind(&placement.work_id)
                .bind(&placement.title)
                .bind(&placement.recipient)
                .bind(similarity_bp)
                .bind(now)
                .execute(db.postgres_pool().expect("postgres"))
                .await?;
            }
        }
    }

    let notice = find_notice(db, &id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("the notice was written and cannot be read back"))?;
    let assignments = assignments_for(db, &id).await?;
    Ok((notice, assignments))
}

fn notice_from_sqlite(row: &sqlx::sqlite::SqliteRow) -> Notice {
    Notice {
        id: row.get("id"),
        reason: row.get("reason"),
        announced_by: row.get("announced_by"),
        announced_at: row.get("announced_at"),
        state: row.get("state"),
        note: row.get("note"),
        completed_at: row.get("completed_at"),
    }
}

fn notice_from_postgres(row: &sqlx::postgres::PgRow) -> Notice {
    Notice {
        id: row.get("id"),
        reason: row.get("reason"),
        announced_by: row.get("announced_by"),
        announced_at: row.get("announced_at"),
        state: row.get("state"),
        note: row.get("note"),
        completed_at: row.get("completed_at"),
    }
}

const NOTICE_COLUMNS: &str =
    "id, reason, announced_by, announced_at, state, note, completed_at";

/// The notice currently announced, if any.
pub async fn open_notice(db: &Database) -> Result<Option<Notice>> {
    match db.backend() {
        Backend::Sqlite => {
            let row = sqlx::query(&format!(
                "SELECT {NOTICE_COLUMNS} FROM evacuation_notices WHERE state = 'announced'"
            ))
            .fetch_optional(db.sqlite_pool().expect("sqlite"))
            .await?;
            Ok(row.as_ref().map(notice_from_sqlite))
        }
        Backend::Postgres => {
            let row = sqlx::query(&format!(
                "SELECT {NOTICE_COLUMNS} FROM evacuation_notices WHERE state = 'announced'"
            ))
            .fetch_optional(db.postgres_pool().expect("postgres"))
            .await?;
            Ok(row.as_ref().map(notice_from_postgres))
        }
    }
}

/// One notice by id, in any state.
pub async fn find_notice(db: &Database, id: &str) -> Result<Option<Notice>> {
    match db.backend() {
        Backend::Sqlite => {
            let row = sqlx::query(&format!(
                "SELECT {NOTICE_COLUMNS} FROM evacuation_notices WHERE id = ?"
            ))
            .bind(id)
            .fetch_optional(db.sqlite_pool().expect("sqlite"))
            .await?;
            Ok(row.as_ref().map(notice_from_sqlite))
        }
        Backend::Postgres => {
            let row = sqlx::query(&format!(
                "SELECT {NOTICE_COLUMNS} FROM evacuation_notices WHERE id = $1"
            ))
            .bind(id)
            .fetch_optional(db.postgres_pool().expect("postgres"))
            .await?;
            Ok(row.as_ref().map(notice_from_postgres))
        }
    }
}

fn assignment_from_sqlite(row: &sqlx::sqlite::SqliteRow) -> Assignment {
    Assignment {
        id: row.get("id"),
        notice_id: row.get("notice_id"),
        work_id: row.get("work_id"),
        title: row.get("title"),
        recipient_instance: row.get("recipient_instance"),
        similarity_bp: row.get("similarity_bp"),
        state: row.get("state"),
        offered_at: row.get("offered_at"),
        settled_at: row.get("settled_at"),
        settle_note: row.get("settle_note"),
    }
}

fn assignment_from_postgres(row: &sqlx::postgres::PgRow) -> Assignment {
    Assignment {
        id: row.get("id"),
        notice_id: row.get("notice_id"),
        work_id: row.get("work_id"),
        title: row.get("title"),
        recipient_instance: row.get("recipient_instance"),
        similarity_bp: row.get("similarity_bp"),
        state: row.get("state"),
        offered_at: row.get("offered_at"),
        settled_at: row.get("settled_at"),
        settle_note: row.get("settle_note"),
    }
}

const ASSIGNMENT_COLUMNS: &str =
    "id, notice_id, work_id, title, recipient_instance, CAST(similarity_bp AS BIGINT) AS similarity_bp, \
     state, offered_at, settled_at, settle_note";
const ASSIGNMENT_COLUMNS_SQLITE: &str =
    "id, notice_id, work_id, title, recipient_instance, similarity_bp, state, offered_at, settled_at, settle_note";

/// Every placement in a notice, offered first, then by title.
pub async fn assignments_for(db: &Database, notice_id: &str) -> Result<Vec<Assignment>> {
    match db.backend() {
        Backend::Sqlite => {
            let rows = sqlx::query(&format!(
                "SELECT {ASSIGNMENT_COLUMNS_SQLITE} FROM evacuation_assignments WHERE notice_id = ? \
                 ORDER BY CASE state WHEN 'offered' THEN 0 WHEN 'accepted' THEN 1 ELSE 2 END, title, work_id"
            ))
            .bind(notice_id)
            .fetch_all(db.sqlite_pool().expect("sqlite"))
            .await?;
            Ok(rows.iter().map(assignment_from_sqlite).collect())
        }
        Backend::Postgres => {
            let rows = sqlx::query(&format!(
                "SELECT {ASSIGNMENT_COLUMNS} FROM evacuation_assignments WHERE notice_id = $1 \
                 ORDER BY CASE state WHEN 'offered' THEN 0 WHEN 'accepted' THEN 1 ELSE 2 END, title, work_id"
            ))
            .bind(notice_id)
            .fetch_all(db.postgres_pool().expect("postgres"))
            .await?;
            Ok(rows.iter().map(assignment_from_postgres).collect())
        }
    }
}

/// Record what a recipient said about one placement.
///
/// `state` is `accepted` or `declined`. Accepting is refused when another recipient has already accepted the same work in
/// this notice: one work, one home, and the refusal names the holder.
pub async fn settle(
    db: &Database,
    assignment_id: &str,
    state: &str,
    note: &str,
    now: &str,
) -> Result<Assignment> {
    if !matches!(state, "accepted" | "declined") {
        return Err(anyhow::anyhow!(
            "a placement settles as 'accepted' or 'declined', not '{state}'"
        ));
    }
    let assignment = find_assignment(db, assignment_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("no such placement"))?;
    if assignment.state != "offered" {
        return Err(anyhow::anyhow!(
            "this placement was already settled as '{}'",
            assignment.state
        ));
    }
    if state == "accepted" {
        // The fork is a fact about the WORK, not about this notice: a plan cannot offer one work twice (`UNIQUE
        // (notice_id, work_id)`), so the way a work ends up with two homes is a *second* evacuation — an instance that
        // announced, closed, and announced again, or one revived under a new operator. So the check spans every notice, and
        // it only bites when the earlier home was a DIFFERENT recipient: recording the same peer twice is one home with two
        // receipts, which is history rather than a fork.
        if let Some((other_notice, holder)) = accepted_holder_anywhere(db, &assignment.work_id).await? {
            if holder != assignment.recipient_instance {
                return Err(anyhow::anyhow!(
                    "{} already took '{}' in evacuation {}; one work gets one home, or the author's page forks",
                    holder,
                    assignment.title,
                    other_notice
                ));
            }
        }
    }

    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "UPDATE evacuation_assignments SET state = ?, settled_at = ?, settle_note = ?
                  WHERE id = ? AND state = 'offered'",
            )
            .bind(state)
            .bind(now)
            .bind(note)
            .bind(assignment_id)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "UPDATE evacuation_assignments SET state = $1, settled_at = $2, settle_note = $3
                  WHERE id = $4 AND state = 'offered'",
            )
            .bind(state)
            .bind(now)
            .bind(note)
            .bind(assignment_id)
            .execute(db.postgres_pool().expect("postgres"))
            .await?;
        }
    }
    find_assignment(db, assignment_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("the placement vanished between write and read"))
}

/// The earliest recipient that accepted this work in any notice, with that notice's id.
///
/// "Earliest" by `settled_at` then id, so the answer is stable: when two rows claim one work, the first one to be recorded is
/// the home and the later one is the fork.
pub async fn accepted_holder_anywhere(
    db: &Database,
    work_id: &str,
) -> Result<Option<(String, String)>> {
    match db.backend() {
        Backend::Sqlite => {
            let row: Option<(String, String)> = sqlx::query_as(
                "SELECT notice_id, recipient_instance FROM evacuation_assignments
                  WHERE work_id = ? AND state = 'accepted'
                  ORDER BY settled_at, id LIMIT 1",
            )
            .bind(work_id)
            .fetch_optional(db.sqlite_pool().expect("sqlite"))
            .await?;
            Ok(row)
        }
        Backend::Postgres => {
            let row: Option<(String, String)> = sqlx::query_as(
                "SELECT notice_id, recipient_instance FROM evacuation_assignments
                  WHERE work_id = $1 AND state = 'accepted'
                  ORDER BY settled_at, id LIMIT 1",
            )
            .bind(work_id)
            .fetch_optional(db.postgres_pool().expect("postgres"))
            .await?;
            Ok(row)
        }
    }
}

/// Which recipient, if any, already accepted this work in this notice.
pub async fn accepted_holder(db: &Database, notice_id: &str, work_id: &str) -> Result<Option<String>> {
    match db.backend() {
        Backend::Sqlite => {
            let holder: Option<String> = sqlx::query_scalar(
                "SELECT recipient_instance FROM evacuation_assignments
                  WHERE notice_id = ? AND work_id = ? AND state = 'accepted' LIMIT 1",
            )
            .bind(notice_id)
            .bind(work_id)
            .fetch_optional(db.sqlite_pool().expect("sqlite"))
            .await?
            .flatten();
            Ok(holder)
        }
        Backend::Postgres => {
            let holder: Option<String> = sqlx::query_scalar(
                "SELECT recipient_instance FROM evacuation_assignments
                  WHERE notice_id = $1 AND work_id = $2 AND state = 'accepted' LIMIT 1",
            )
            .bind(notice_id)
            .bind(work_id)
            .fetch_optional(db.postgres_pool().expect("postgres"))
            .await?
            .flatten();
            Ok(holder)
        }
    }
}

/// One placement by id.
pub async fn find_assignment(db: &Database, id: &str) -> Result<Option<Assignment>> {
    match db.backend() {
        Backend::Sqlite => {
            let row = sqlx::query(&format!(
                "SELECT {ASSIGNMENT_COLUMNS_SQLITE} FROM evacuation_assignments WHERE id = ?"
            ))
            .bind(id)
            .fetch_optional(db.sqlite_pool().expect("sqlite"))
            .await?;
            Ok(row.as_ref().map(assignment_from_sqlite))
        }
        Backend::Postgres => {
            let row = sqlx::query(&format!(
                "SELECT {ASSIGNMENT_COLUMNS} FROM evacuation_assignments WHERE id = $1"
            ))
            .bind(id)
            .fetch_optional(db.postgres_pool().expect("postgres"))
            .await?;
            Ok(row.as_ref().map(assignment_from_postgres))
        }
    }
}

/// The receipt: who took what, and what was declined.
pub async fn receipt(db: &Database, notice_id: &str) -> Result<Receipt> {
    let assignments = assignments_for(db, notice_id).await?;
    let mut receipt = Receipt::default();
    for assignment in assignments {
        match assignment.state.as_str() {
            "accepted" => receipt.accepted.push(Acceptance {
                work_id: assignment.work_id,
                title: assignment.title,
                recipient: assignment.recipient_instance,
            }),
            "declined" => receipt.declined.push(Unplaced {
                work_id: assignment.work_id,
                title: assignment.title,
                reason: assignment
                    .settle_note
                    .unwrap_or_else(|| "declined without a note".to_owned()),
            }),
            _ => {}
        }
    }
    Ok(receipt)
}

/// Withdraw the open announcement, or complete it.
///
/// Withdrawing keeps the notice and its placements: an evacuation that was called off is a fact about the instance's history,
/// and deleting it would leave the receipt describing an event with no announcement behind it.
pub async fn close(db: &Database, reason: &str, now: &str) -> Result<Notice> {
    if !matches!(reason, "withdrawn" | "completed") {
        return Err(anyhow::anyhow!(
            "an announcement closes as 'withdrawn' or 'completed', not '{reason}'"
        ));
    }
    let open = open_notice(db)
        .await?
        .ok_or_else(|| anyhow::anyhow!("no evacuation is announced"))?;
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query("UPDATE evacuation_notices SET state = ?, completed_at = ? WHERE id = ?")
                .bind(reason)
                .bind(now)
                .bind(&open.id)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query("UPDATE evacuation_notices SET state = $1, completed_at = $2 WHERE id = $3")
                .bind(reason)
                .bind(now)
                .bind(&open.id)
                .execute(db.postgres_pool().expect("postgres"))
                .await?;
        }
    }
    find_notice(db, &open.id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("the notice vanished between write and read"))
}
