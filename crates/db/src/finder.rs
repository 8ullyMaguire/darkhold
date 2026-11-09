//! M45-34 — the bounty store.
//!
//! The credits are the hard part, so the order of operations is deliberate:
//!
//! 1. **Reserve on open.** `credit_holds` is the existing escrow primitive; a bounty with no hold could be
//!    accepted by somebody who has already spent the credits elsewhere.
//! 2. **Compare-and-set on accept.** Two people accepting different answers at the same moment must produce one
//!    winner; `WHERE state = 'open'` plus `rows_affected` is what makes that a fact rather than a hope.
//! 3. **Pay after the winner is fixed, idempotently.** The payout's idempotency key is derived from the winning
//!    proposal, so a retried request re-uses the same transaction instead of paying twice.
//! 4. **Release on cancel or expiry.** A cancelled bounty that kept its hold would quietly freeze someone's
//!    credits forever.

use commons_bounty::{may_accept, may_propose, validate_new, Bounty, BountyState, Proposal};

use crate::{sql_owned, Backend, Database, Result};

/// A new bounty as the route received it.
#[derive(Debug, Clone)]
pub struct NewBounty {
    pub description: String,
    pub fandom: Option<String>,
    pub budget: i64,
    pub closes_at: String,
}

/// What `create` did.
#[derive(Debug, Clone, PartialEq)]
pub enum Created {
    Bounty(Bounty),
    /// Refused, with the sentence explaining it.
    Refused(String),
}

/// What `accept` did.
#[derive(Debug, Clone, PartialEq)]
pub enum Accepted {
    /// Paid, with the transaction id.
    Paid {
        bounty: Box<Bounty>,
        transaction_id: String,
    },
    /// Refused, with the sentence.
    Refused(String),
}

fn now_rfc3339() -> String {
    crate::identity::now_rfc3339()
}

/// Credits an account can commit right now: what it holds, minus holds that are still standing.
///
/// # Errors
/// As the store.
pub async fn available_credits(db: &Database, account: &str) -> Result<i64> {
    let balance_sql = sql_owned(
        db,
        "SELECT COALESCE(SUM(amount_bp), 0) FROM credit_entries WHERE account = ? AND bucket = 'earned'"
            .to_owned(),
        "SELECT COALESCE(SUM(amount_bp), 0) FROM credit_entries WHERE account = $1 AND bucket = 'earned'"
            .to_owned(),
    );
    let holds_sql = sql_owned(
        db,
        "SELECT COALESCE(SUM(amount), 0) FROM credit_holds \
          WHERE account = ? AND released_at IS NULL AND captured_at IS NULL AND expires_at > ?"
            .to_owned(),
        "SELECT COALESCE(SUM(amount), 0) FROM credit_holds \
          WHERE account = $1 AND released_at IS NULL AND captured_at IS NULL AND expires_at > $2"
            .to_owned(),
    );
    let now = now_rfc3339();
    let (balance, held): (i64, i64) = match db.backend() {
        Backend::Sqlite => {
            let pool = db.sqlite_pool().expect("sqlite handle");
            let balance: i64 = sqlx::query_scalar(&balance_sql)
                .bind(account)
                .fetch_one(pool)
                .await?;
            let held: i64 = sqlx::query_scalar(&holds_sql)
                .bind(account)
                .bind(&now)
                .fetch_one(pool)
                .await?;
            (balance, held)
        }
        Backend::Postgres => {
            let pool = db.postgres_pool().expect("postgres handle");
            let balance: i64 = sqlx::query_scalar(&balance_sql)
                .bind(account)
                .fetch_one(pool)
                .await?;
            let held: i64 = sqlx::query_scalar(&holds_sql)
                .bind(account)
                .bind(&now)
                .fetch_one(pool)
                .await?;
            (balance, held)
        }
    };
    Ok(balance - held)
}

/// Open a bounty, reserving its budget.
///
/// # Errors
/// As the store.
pub async fn create(db: &Database, asker: &str, new: &NewBounty) -> Result<Created> {
    let now = now_rfc3339();
    // The close date is clamped to the bounty's own limit, so the hold cannot outlive it.
    if let Err(reason) = validate_new(&new.description, new.budget, &now, &new.closes_at) {
        return Ok(Created::Refused(reason));
    }
    let available = available_credits(db, asker).await?;
    if available < new.budget {
        return Ok(Created::Refused(format!(
            "budget: {available} credits available, {0} promised — a bounty can only offer what it could pay",
            new.budget
        )));
    }
    let id = uuid::Uuid::new_v4().to_string();
    let ttl = {
        let closes = time::OffsetDateTime::parse(
            new.closes_at.trim(),
            &time::format_description::well_known::Rfc3339,
        )
        .map_err(|error| sqlx::Error::Decode(Box::new(error)))?;
        let now_ts = time::OffsetDateTime::now_utc();
        (closes - now_ts).whole_seconds().max(60)
    };
    let hold_id = crate::economy::reserve_hold(db, asker, &id, new.budget, ttl).await?;

    let sql = sql_owned(
        db,
        "INSERT INTO finder_bounties \
           (id, asker_account_id, description, fandom, budget, state, closes_at, created_at, hold_id) \
         VALUES (?, ?, ?, ?, ?, 'open', ?, ?, ?)"
            .to_owned(),
        "INSERT INTO finder_bounties \
           (id, asker_account_id, description, fandom, budget, state, closes_at, created_at, hold_id) \
         VALUES ($1, $2::uuid, $3, $4, $5, 'open', $6, $7, $8)"
            .to_owned(),
    );
    let description = new.description.trim().to_owned();
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(&id)
                .bind(asker)
                .bind(&description)
                .bind(&new.fandom)
                .bind(new.budget)
                .bind(&new.closes_at)
                .bind(&now)
                .bind(&hold_id)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(&id)
                .bind(asker)
                .bind(&description)
                .bind(&new.fandom)
                .bind(new.budget)
                .bind(&new.closes_at)
                .bind(&now)
                .bind(&hold_id)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }
    match get(db, &id).await? {
        Some(bounty) => Ok(Created::Bounty(bounty)),
        None => Ok(Created::Refused(
            "the bounty vanished between insert and read".to_owned(),
        )),
    }
}

/// One bounty by id.
///
/// # Errors
/// As the store.
pub async fn get(db: &Database, id: &str) -> Result<Option<Bounty>> {
    let sql = sql_owned(
        db,
        "SELECT id, asker_account_id, description, fandom, budget, state, closes_at, created_at, \
                accepted_proposal_id, hold_id FROM finder_bounties WHERE id = ?"
            .to_owned(),
        "SELECT id, asker_account_id::text AS asker_account_id, description, fandom, budget, state, closes_at, \
                created_at, accepted_proposal_id, hold_id FROM finder_bounties WHERE id = $1"
            .to_owned(),
    );
    let row: Option<BountyRow> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(id)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(id)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(row.and_then(bounty_from_row))
}

/// The column tuple `finder_bounties` reads into, named once because both queries use it.
type BountyRow = (
    String,
    String,
    String,
    Option<String>,
    i64,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
);

fn bounty_from_row(row: BountyRow) -> Option<Bounty> {
    let (id, asker, description, fandom, budget, state, closes_at, created_at, accepted, hold) =
        row;
    Some(Bounty {
        id,
        asker_account_id: asker,
        description,
        subject: fandom,
        budget,
        state: BountyState::parse(&state)?,
        closes_at,
        created_at,
        accepted_proposal_id: accepted,
        hold_id: hold,
    })
}

/// Open bounties, newest first, optionally filtered by fandom (case-insensitive).
///
/// # Errors
/// As the store.
pub async fn open_bounties(db: &Database, fandom: Option<&str>) -> Result<Vec<Bounty>> {
    let now = now_rfc3339();
    let filter = fandom.map(|value| value.trim().to_ascii_lowercase());
    let sql = sql_owned(
        db,
        "SELECT id, asker_account_id, description, fandom, budget, state, closes_at, created_at, \
                accepted_proposal_id, hold_id FROM finder_bounties \
          WHERE state = 'open' AND closes_at > ? AND (? IS NULL OR lower(fandom) = ?) \
          ORDER BY created_at DESC, id".to_owned(),
        "SELECT id, asker_account_id::text AS asker_account_id, description, fandom, budget, state, closes_at, \
                created_at, accepted_proposal_id, hold_id FROM finder_bounties \
          WHERE state = 'open' AND closes_at > $1 AND ($2::text IS NULL OR lower(fandom) = $2::text) \
          ORDER BY created_at DESC, id"
            .to_owned(),
    );
    let rows: Vec<BountyRow> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(&now)
                .bind(&filter)
                .bind(&filter)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(&now)
                .bind(&filter)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows.into_iter().filter_map(bounty_from_row).collect())
}

/// Answer a bounty with a work.
///
/// # Errors
/// As the store.
pub async fn propose(
    db: &Database,
    bounty_id: &str,
    proposer: &str,
    work_id: &str,
    note: &str,
) -> Result<std::result::Result<Proposal, String>> {
    let Some(bounty) = get(db, bounty_id).await? else {
        return Ok(Err("bounty: no such bounty".to_owned()));
    };
    if let Err(reason) = may_propose(bounty.state, &bounty.asker_account_id, proposer) {
        return Ok(Err(reason));
    }
    // The proposal has to name a work that exists: a typo'd id would be an answer nobody can open.
    let exists_sql = sql_owned(
        db,
        "SELECT 1 FROM works WHERE id = ?".to_owned(),
        "SELECT 1 FROM works WHERE id = $1::uuid".to_owned(),
    );
    let exists: Option<i32> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&exists_sql)
                .bind(work_id)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&exists_sql)
                .bind(work_id)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    if exists.is_none() {
        return Ok(Err("work_id: no such work".to_owned()));
    }

    let id = uuid::Uuid::new_v4().to_string();
    let now = now_rfc3339();
    let sql = sql_owned(
        db,
        "INSERT INTO finder_proposals (id, bounty_id, proposer_account_id, work_id, note, state, created_at) \
         VALUES (?, ?, ?, ?, ?, 'proposed', ?) \
         ON CONFLICT (bounty_id, proposer_account_id, work_id) DO NOTHING".to_owned(),
        "INSERT INTO finder_proposals (id, bounty_id, proposer_account_id, work_id, note, state, created_at) \
         VALUES ($1, $2, $3::uuid, $4::uuid, $5, 'proposed', $6) \
         ON CONFLICT (bounty_id, proposer_account_id, work_id) DO NOTHING"
            .to_owned(),
    );
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(&sql)
            .bind(&id)
            .bind(bounty_id)
            .bind(proposer)
            .bind(work_id)
            .bind(note.trim())
            .bind(&now)
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(&sql)
            .bind(&id)
            .bind(bounty_id)
            .bind(proposer)
            .bind(work_id)
            .bind(note.trim())
            .bind(&now)
            .execute(db.postgres_pool().expect("postgres handle"))
            .await?
            .rows_affected(),
    };
    if affected == 0 {
        // The unique key refused a repeat: the same person naming the same work twice is one answer.
        return Ok(Err(
            "work_id: you already named that work on this bounty".to_owned()
        ));
    }
    Ok(Ok(Proposal {
        id,
        bounty_id: bounty_id.to_owned(),
        proposer_account_id: proposer.to_owned(),
        subject_id: work_id.to_owned(),
        note: note.trim().to_owned(),
        state: "proposed".to_owned(),
        created_at: now,
    }))
}

/// Proposals on a bounty, oldest first.
///
/// # Errors
/// As the store.
pub async fn proposals(db: &Database, bounty_id: &str) -> Result<Vec<Proposal>> {
    let sql = sql_owned(
        db,
        "SELECT id, bounty_id, proposer_account_id, work_id, note, state, created_at \
           FROM finder_proposals WHERE bounty_id = ? ORDER BY created_at, id".to_owned(),
        "SELECT id, bounty_id, proposer_account_id::text AS proposer_account_id, work_id::text AS work_id, \
                note, state, created_at FROM finder_proposals WHERE bounty_id = $1 ORDER BY created_at, id"
            .to_owned(),
    );
    type ProposalRow = (String, String, String, String, String, String, String);
    let rows: Vec<ProposalRow> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(bounty_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(bounty_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows
        .into_iter()
        .map(
            |(id, bounty_id, proposer, work_id, note, state, created_at)| Proposal {
                id,
                bounty_id,
                proposer_account_id: proposer,
                subject_id: work_id,
                note,
                state,
                created_at,
            },
        )
        .collect())
}

/// Accept one proposal and pay its proposer.
///
/// # Errors
/// As the store.
pub async fn accept(
    db: &Database,
    bounty_id: &str,
    proposal_id: &str,
    asker: &str,
) -> Result<Accepted> {
    let Some(bounty) = get(db, bounty_id).await? else {
        return Ok(Accepted::Refused("bounty: no such bounty".to_owned()));
    };
    if bounty.asker_account_id != asker {
        // Not "forbidden": confirming that somebody else's bounty exists is not this route's business.
        return Ok(Accepted::Refused("bounty: no such bounty".to_owned()));
    }
    let proposals = proposals(db, bounty_id).await?;
    let Some(proposal) = proposals.iter().find(|row| row.id == proposal_id) else {
        return Ok(Accepted::Refused("proposal: no such proposal".to_owned()));
    };
    let now = now_rfc3339();
    if let Err(reason) = may_accept(
        bounty.state,
        &bounty.closes_at,
        &bounty.asker_account_id,
        &proposal.proposer_account_id,
        &now,
    ) {
        return Ok(Accepted::Refused(reason));
    }

    // One winner: the state predicate is the lock.
    let cas_sql = sql_owned(
        db,
        "UPDATE finder_bounties SET state = 'accepted', closed_at = ?, accepted_proposal_id = ? \
          WHERE id = ? AND state = 'open'"
            .to_owned(),
        "UPDATE finder_bounties SET state = 'accepted', closed_at = $1, accepted_proposal_id = $2 \
          WHERE id = $3 AND state = 'open'"
            .to_owned(),
    );
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(&cas_sql)
            .bind(&now)
            .bind(proposal_id)
            .bind(bounty_id)
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(&cas_sql)
            .bind(&now)
            .bind(proposal_id)
            .bind(bounty_id)
            .execute(db.postgres_pool().expect("postgres handle"))
            .await?
            .rows_affected(),
    };
    if affected == 0 {
        return Ok(Accepted::Refused(
            "bounty: it was closed while you were deciding — somebody else moved it".to_owned(),
        ));
    }

    // The winner is fixed; mark it and reject the rest.
    let mark_winner = sql_owned(
        db,
        "UPDATE finder_proposals SET state = 'accepted' WHERE id = ?".to_owned(),
        "UPDATE finder_proposals SET state = 'accepted' WHERE id = $1".to_owned(),
    );
    let reject_others = sql_owned(
        db,
        "UPDATE finder_proposals SET state = 'rejected' WHERE bounty_id = ? AND id <> ? AND state = 'proposed'"
            .to_owned(),
        "UPDATE finder_proposals SET state = 'rejected' WHERE bounty_id = $1 AND id <> $2 AND state = 'proposed'"
            .to_owned(),
    );
    match db.backend() {
        Backend::Sqlite => {
            let pool = db.sqlite_pool().expect("sqlite handle");
            sqlx::query(&mark_winner)
                .bind(proposal_id)
                .execute(pool)
                .await?;
            sqlx::query(&reject_others)
                .bind(bounty_id)
                .bind(proposal_id)
                .execute(pool)
                .await?;
        }
        Backend::Postgres => {
            let pool = db.postgres_pool().expect("postgres handle");
            sqlx::query(&mark_winner)
                .bind(proposal_id)
                .execute(pool)
                .await?;
            sqlx::query(&reject_others)
                .bind(bounty_id)
                .bind(proposal_id)
                .execute(pool)
                .await?;
        }
    }

    // Pay. The key is the winning proposal, so a retry after a crash re-uses the transaction.
    let key = format!("finder-payout:{proposal_id}");
    let entries = vec![
        (
            bounty.asker_account_id.clone(),
            "earned".to_owned(),
            -bounty.budget,
        ),
        (
            proposal.proposer_account_id.clone(),
            "earned".to_owned(),
            bounty.budget,
        ),
    ];
    let transaction_id = crate::economy::post_transaction(
        db,
        darkhold_domain::economy::TxnType::Spend,
        &key,
        &format!("finder_bounty:{bounty_id}"),
        &entries,
    )
    .await?;
    if let Some(hold) = bounty.hold_id.as_deref() {
        crate::economy::capture_hold(db, hold, bounty.budget).await?;
    }

    match get(db, bounty_id).await? {
        Some(updated) => Ok(Accepted::Paid {
            bounty: Box::new(updated),
            transaction_id,
        }),
        None => Ok(Accepted::Refused(
            "the bounty vanished while paying it".to_owned(),
        )),
    }
}

/// Cancel an open bounty, releasing its credits.
///
/// # Errors
/// As the store.
pub async fn cancel(db: &Database, bounty_id: &str, asker: &str) -> Result<Option<Bounty>> {
    let Some(bounty) = get(db, bounty_id).await? else {
        return Ok(None);
    };
    if bounty.asker_account_id != asker || bounty.state != BountyState::Open {
        return Ok(None);
    }
    let now = now_rfc3339();
    let sql = sql_owned(
        db,
        "UPDATE finder_bounties SET state = 'cancelled', closed_at = ? WHERE id = ? AND state = 'open'"
            .to_owned(),
        "UPDATE finder_bounties SET state = 'cancelled', closed_at = $1 WHERE id = $2 AND state = 'open'"
            .to_owned(),
    );
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(&sql)
            .bind(&now)
            .bind(bounty_id)
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(&sql)
            .bind(&now)
            .bind(bounty_id)
            .execute(db.postgres_pool().expect("postgres handle"))
            .await?
            .rows_affected(),
    };
    if affected == 0 {
        return Ok(None);
    }
    if let Some(hold) = bounty.hold_id.as_deref() {
        crate::economy::release_hold(db, hold).await?;
    }
    get(db, bounty_id).await
}

/// Mark bounties past their close date as expired and release their holds.
///
/// Idempotent, and safe to run on a loop: the state predicate means a second run finds nothing.
///
/// # Errors
/// As the store.
pub async fn sweep_expired(db: &Database) -> Result<usize> {
    let now = now_rfc3339();
    let select_sql = sql_owned(
        db,
        "SELECT id, hold_id FROM finder_bounties WHERE state = 'open' AND closes_at <= ?"
            .to_owned(),
        "SELECT id, hold_id FROM finder_bounties WHERE state = 'open' AND closes_at <= $1"
            .to_owned(),
    );
    type ExpiredRow = (String, Option<String>);
    let rows: Vec<ExpiredRow> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&select_sql)
                .bind(&now)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&select_sql)
                .bind(&now)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    let mut expired = 0;
    for (id, hold_id) in rows {
        let sql = sql_owned(
            db,
            "UPDATE finder_bounties SET state = 'expired', closed_at = ? WHERE id = ? AND state = 'open'"
                .to_owned(),
            "UPDATE finder_bounties SET state = 'expired', closed_at = $1 WHERE id = $2 AND state = 'open'"
                .to_owned(),
        );
        let affected = match db.backend() {
            Backend::Sqlite => sqlx::query(&sql)
                .bind(&now)
                .bind(&id)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?
                .rows_affected(),
            Backend::Postgres => sqlx::query(&sql)
                .bind(&now)
                .bind(&id)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?
                .rows_affected(),
        };
        if affected > 0 {
            expired += 1;
            if let Some(hold) = hold_id.as_deref() {
                crate::economy::release_hold(db, hold).await?;
            }
        }
    }
    Ok(expired)
}

