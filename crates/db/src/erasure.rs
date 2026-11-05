//! M45-51 — subject access and erasure.
//!
//! Spec: `docs/plans/m45-51-subject-access-and-erasure.md`.
//!
//! ## The cascade is the schema's job, not this module's
//!
//! Parsing every `CREATE TABLE`/`REFERENCES` in `migrations/sqlite/` shows **71 tables** whose
//! foreign-key chain reaches `accounts`, and **exactly one** column into `accounts` that does
//! not cascade: `jobs.requested_by`, `ON DELETE SET NULL`, because a job's audit trail must
//! survive the requester who asked for it.
//!
//! **That measurement was incomplete, and the omission was the leak.** Counting only tables
//! already in the FK graph cannot see a table whose `account_id` is a plain `TEXT`/`UUID` column
//! with no `REFERENCES` clause at all. Measured directly — every column named `account_id` or
//! `*_account`, then `PRAGMA foreign_key_list` per table — there are **18 such columns across 16
//! tables** (M45-64). `DELETE FROM accounts` touches none of them: the account goes and a
//! `tasting_responses` row still records which way that person voted, under an id that resolves
//! to nobody. See [`FK_FREE_ACCOUNT_COLUMNS`], which is that leak closed, and which is asserted
//! against the schema by `tests/erasure_covers_fk_free_accounts.rs`.
//!
//! **Every column in that table now has a treatment that runs.** `supply_routing.account_id` was the
//! last `DeferredTo`, and M45-58 closed it without a migration: the routing row survives (§58.3 — an
//! abandoned routing is evidence the reader was asked) and the account link is replaced with a
//! per-row key `erased:<row id>`, because the column is `NOT NULL` and a shared placeholder would
//! re-link every severed row of one person to the others. See [`Treatment::Rekey`].
//!
//! So [`erase_account`] is effectively a single `DELETE FROM accounts`. It is deliberately NOT
//! an application-side list of 71 tables: such a list is 71 entries to keep in sync with the
//! migrations, and it fails *silently* — the day a migration adds a table, the list is stale
//! and the erasure leaks. This project has already shipped `did_not_finish` beside
//! `reading_history_entry` as two tables where one richer one existed; a second parallel
//! erasure path would be that mistake with legal consequences.
//!
//! What this module owns is the part the schema cannot: telling the reader **in advance** what
//! will be destroyed, and assembling what will be disclosed.
//!
//! ## Why `plan_erasure` exists at all
//!
//! `works.owner_pseud_id` is `ON DELETE CASCADE`. Erasing an account therefore **deletes the
//! works that account authored** — for an archive of fanfic, a serious and possibly
//! regrettable consequence. A `DELETE` that silently removes published writing because someone
//! asked to be forgotten is not a small surprise, so the route refuses without `force` and
//! reports the count. See the spec's §2a step 2.

use crate::{sql_owned, Backend, Database, Result};
use serde::Serialize;
use sqlx::FromRow;

/// One reader-private row, as disclosed.
#[derive(Debug, Clone, Serialize, PartialEq, FromRow)]
pub struct BookmarkRow {
    pub id: String,
    pub subject_type: String,
    pub subject_id: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, FromRow)]
pub struct ReadingProgressRow {
    pub id: String,
    pub subject_type: String,
    pub subject_id: String,
    pub position_permille: i64,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, FromRow)]
pub struct DidNotFinishRow {
    pub id: String,
    pub pseud_id: String,
    pub work_id: String,
    pub reason: String,
}

/// Everything held about one reader, in the shape a subject-access response takes.
///
/// ## Field naming is load-bearing
///
/// This struct serialises to a JSON object, and the test
/// `subject_data_never_carries_a_numeric_resonance_score` walks the serialised keys against
/// [`darkhold_domain::analytics::FORBIDDEN_SCOPE_NAMES`] — 13 names that must never be
/// readable. Every field here is named from the *reader-facing* vocabulary
/// (`bookmarks`, `reading_progress`, `did_not_finish`) and deliberately avoids the analytics
/// namespace entirely.
///
/// In particular there is **no resonance field**, because `ab.resonance_numeric` is forbidden
/// and `OwnResonanceLabel` exists so that the *label* is the only form ever shown. The
/// temptation to add the number "for debugging" is the obvious future mistake, and a field
/// called `resonance_score` would fail the forbidden-key walk by name.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct SubjectData {
    pub account_id: String,
    pub email: Option<String>,
    pub handles: Vec<String>,
    pub bookmarks: Vec<BookmarkRow>,
    pub reading_progress: Vec<ReadingProgressRow>,
    pub did_not_finish: Vec<DidNotFinishRow>,
}

/// What an erasure would destroy, counted before it happens.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ErasurePlan {
    pub account_id: String,
    pub handle: Option<String>,
    /// Private row counts by table name, so the route can render "12 bookmarks, 3 in progress"
    /// without hardcoding a table list of its own.
    pub private_rows: Vec<(String, i64)>,
    /// Rows in account-keyed tables that no foreign key reaches (M45-64), counted by table: what
    /// the cascade would leave behind and the erasure removes explicitly. `DeferredTo` entries are
    /// **not** counted here, because counting them would claim an erasure that does not happen.
    pub unlinked_rows: Vec<(String, i64)>,
    pub published_work_count: i64,
    pub open_export_count: i64,
}

impl ErasurePlan {
    /// The count for one table. `0` for a table with no rows, so a caller never has to
    /// distinguish "absent" from "zero" — a distinction with no meaning here, and exactly the
    /// sort of thing that renders as `undefined` in a client.
    pub fn private_row_count(&self, table: &str) -> i64 {
        self.private_rows
            .iter()
            .find(|(t, _)| t == table)
            .map(|(_, n)| *n)
            .unwrap_or(0)
    }

    /// Whether the plan would destroy anything at all. A reader with nothing stored should be
    /// told the account itself is all that would go, not handed a list of zeroes.
    pub fn has_private_rows(&self) -> bool {
        self.private_rows.iter().any(|(_, n)| *n > 0)
    }

    pub fn total_private_rows(&self) -> i64 {
        self.private_rows.iter().map(|(_, n)| n).sum()
    }

    pub fn total_unlinked_rows(&self) -> i64 {
        self.unlinked_rows.iter().map(|(_, n)| n).sum()
    }
}

/// The tables an erasure reports on, in the order a reader would hear them.
///
/// "your bookmarks, what you were reading, the things you marked as not for you" is a
/// sentence; alphabetical is a dump. These are the *reported* tables, not the *deleted* ones —
/// deletion is the cascade's job (see the module docs).
const PRIVATE_TABLES: &[&str] = &["bookmarks", "reading_progress", "did_not_finish"];

// ─────────────────────────────────────────────────────────────────────────────
// The columns outside the foreign-key graph (M45-64, M65-10)
// ─────────────────────────────────────────────────────────────────────────────

/// What the store type of an account column is, which decides the SQL.
///
/// A `&str` bound to a `uuid` column is `incorrect binary data format` on PostgreSQL, so the two
/// kinds cannot share one statement. TEXT columns compare directly; UUID columns compare as text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnKind {
    Text,
    Uuid,
}

/// What an erasure does to one account-keyed column that has no foreign key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Treatment {
    /// The row is about that person. It goes.
    Delete,
    /// The row must survive — a financial record, an audit trail — and its account link is
    /// cleared. Only legal where the column is nullable.
    Sever,
    /// The row must survive and the column is `NOT NULL`, so the link is replaced with a **per-row**
    /// key rather than cleared. Distinct from `Sever` because a nullable column can say "nobody";
    /// this one has to say "nobody", and a shared placeholder would re-link every severed row of the
    /// same person to each other — a pseudonym is still personal data if it can be joined on.
    Rekey,
    /// Must be severed and cannot be yet: the column is `NOT NULL` and rebuilding it is that
    /// requirement's work. **Named with its owner, so it is a tracked gap and not an omission.**
    DeferredTo(&'static str),
}

/// One account-keyed column that sits outside the foreign-key graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnlinkedColumn {
    pub table: &'static str,
    pub column: &'static str,
    pub kind: ColumnKind,
    pub treatment: Treatment,
    /// Why this treatment, in one line — because the next reader of this table is deciding
    /// whether the entry is right, and a bare `Delete` does not say whether it was measured.
    pub why: &'static str,
}

/// Every account-keyed column that `DELETE FROM accounts` does **not** reach.
///
/// This list is load-bearing in both directions: `tests/erasure_covers_fk_free_accounts.rs` parses
/// the migrations, collects every account-keyed column outside the FK graph, and fails when one is
/// missing here (a new table cannot be added without deciding what erasure does to it) **and** when
/// an entry here names a column the schema no longer has (the list cannot rot in place).
pub const FK_FREE_ACCOUNT_COLUMNS: &[UnlinkedColumn] = &[
    UnlinkedColumn { table: "directory_votes", column: "account_id", kind: ColumnKind::Text,
        treatment: Treatment::Delete, why: "a vote is a personal act, not an audit record" },
    UnlinkedColumn { table: "interaction_warmth", column: "account_id", kind: ColumnKind::Uuid,
        treatment: Treatment::Delete, why: "warmth between two accounts is a relationship, and a relationship with an erased account does not exist" },
    UnlinkedColumn { table: "interaction_warmth", column: "author_account", kind: ColumnKind::Uuid,
        treatment: Treatment::Delete, why: "same row as the reader's side; the second column is the same evidence" },
    UnlinkedColumn { table: "payment_events", column: "account_id", kind: ColumnKind::Uuid,
        treatment: Treatment::Sever, why: "a payment record is a financial record and must survive its payer; the identity is cleared, the amount and date are not" },
    UnlinkedColumn { table: "payment_events", column: "author_account", kind: ColumnKind::Uuid,
        treatment: Treatment::Sever, why: "the payee side of the same record, on the same reasoning" },
    UnlinkedColumn { table: "vanguard_roles", column: "account_id", kind: ColumnKind::Uuid,
        treatment: Treatment::Delete, why: "a role is held by a person; the role ends with them" },
    UnlinkedColumn { table: "curator_rewards", column: "account_id", kind: ColumnKind::Uuid,
        treatment: Treatment::Delete, why: "a reward is addressed to a person and carries no accounting requirement of its own" },
    UnlinkedColumn { table: "curator_roles", column: "account_id", kind: ColumnKind::Text,
        treatment: Treatment::Delete, why: "a role held by a person" },
    UnlinkedColumn { table: "author_media_preferences", column: "account_id", kind: ColumnKind::Text,
        treatment: Treatment::Delete, why: "a preference is the clearest thing to delete: restoring it would act against the stated wish" },
    UnlinkedColumn { table: "targeted_bounties", column: "account_id", kind: ColumnKind::Text,
        treatment: Treatment::Delete, why: "a bounty is addressed to one person; it has no meaning for anyone else" },
    UnlinkedColumn { table: "category_votes", column: "account_id", kind: ColumnKind::Text,
        treatment: Treatment::Delete, why: "a vote is a personal act" },
    UnlinkedColumn { table: "entry_moderation_votes", column: "account_id", kind: ColumnKind::Text,
        treatment: Treatment::Delete, why: "a jury vote is a personal act; the decision it contributed to is recorded elsewhere" },
    UnlinkedColumn { table: "exchange_signals", column: "account_id", kind: ColumnKind::Text,
        treatment: Treatment::Delete, why: "a behavioural signal about one reader" },
    UnlinkedColumn { table: "tasting_samples", column: "account_id", kind: ColumnKind::Text,
        treatment: Treatment::Delete, why: "which pairs were sampled for this reader is a behavioural record" },
    UnlinkedColumn { table: "tasting_responses", column: "account_id", kind: ColumnKind::Text,
        treatment: Treatment::Delete, why: "a taste judgement is the most personal thing in the tree; deleting the row, not rekeying it, is the only answer that does not keep the preference alive" },
    UnlinkedColumn { table: "trust_events", column: "account_id", kind: ColumnKind::Uuid,
        treatment: Treatment::Delete, why: "the karma ledger is derived from acts; the account's rows go with it" },
    UnlinkedColumn { table: "trust_events", column: "actor_account_id", kind: ColumnKind::Uuid,
        treatment: Treatment::Delete, why: "the same row seen from the actor side" },
    UnlinkedColumn { table: "supply_routing", column: "account_id", kind: ColumnKind::Text,
        treatment: Treatment::Rekey,
        why: "§58.3 keeps the routing row forever: an abandoned routing is the evidence the reader was asked, and deleting it would hide that. §51 still requires the person to leave, so the link becomes a per-row key — 'erased:<row id>' — unlinkable to the reader and to their other rows, and the column stays NOT NULL so nothing had to be rebuilt" },
    UnlinkedColumn { table: "badge_award", column: "account_id", kind: ColumnKind::Text,
        treatment: Treatment::Delete, why: "§9.7.1 makes recognition episodic and personal; a badge held by an erased account is not an audit record of anything, and M47-14's table was the leak this list found" },
    UnlinkedColumn { table: "oidc_links", column: "account_id", kind: ColumnKind::Text,
        treatment: Treatment::Delete, why: "§18/W2: the row says 'this estate subject is this account'. Erasing the account and keeping the link would leave a way back in that no session, password or erase ever removed — the identity the person erased from is still claimed by a subject the estate still vouches for" },
    UnlinkedColumn { table: "external_seeds", column: "account_id", kind: ColumnKind::Text,
        treatment: Treatment::Delete, why: "M45-67: a list of the works a reader wants more of is one of the most personal things they can paste, and it is not an audit record of anything — erasing the account must take the list with it rather than leave it under an id that no longer resolves to anybody" },
    UnlinkedColumn { table: "module_user_state", column: "account_id", kind: ColumnKind::Text,
        treatment: Treatment::Delete, why: "M65-10: a module preference belongs to the person; the snapshot policy's `rekey_account` is a SNAPSHOT treatment (11.16.3b), not this one — live erasure deletes the account, so there is nothing to rekey to and a preference must not survive" },
];

// ─────────────────────────────────────────────────────────────────────────────
// Subject access
// ─────────────────────────────────────────────────────────────────────────────

/// Everything held about `account_id`.
///
/// Every section is a `Vec`, never `Option<Vec<_>>`: a client that has to branch on null-ness
/// is a client with a bug, and an empty reader must still receive `[]`.
pub async fn subject_data(db: &Database, account_id: &str) -> Result<SubjectData> {
    let sql = db.sql(
        "SELECT email FROM accounts WHERE id = ?",
        "SELECT email FROM accounts WHERE id::text = $1",
    );
    let email: Option<String> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar::<_, String>(&sql)
                .bind(account_id)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar::<_, String>(&sql)
                .bind(account_id)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };

    let sql = db.sql(
        "SELECT handle FROM pseuds WHERE account_id = ? ORDER BY created_at",
        "SELECT handle FROM pseuds WHERE account_id::text = $1 ORDER BY created_at",
    );
    let handles: Vec<String> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar::<_, String>(&sql)
                .bind(account_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar::<_, String>(&sql)
                .bind(account_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };

    let sql = db.sql(
        "SELECT id, subject_type, subject_id, created_at FROM bookmarks \
         WHERE account_id = ? ORDER BY created_at, id",
        "SELECT id::text AS id, subject_type, subject_id::text AS subject_id, created_at \
         FROM bookmarks WHERE account_id::text = $1 ORDER BY created_at, id",
    );
    let bookmarks: Vec<BookmarkRow> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as::<_, BookmarkRow>(&sql)
                .bind(account_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as::<_, BookmarkRow>(&sql)
                .bind(account_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };

    let sql = db.sql(
        "SELECT id, subject_type, subject_id, position_permille, updated_at \
         FROM reading_progress WHERE account_id = ? ORDER BY updated_at, id",
        "SELECT id::text AS id, subject_type, subject_id::text AS subject_id, \
         position_permille, updated_at FROM reading_progress \
         WHERE account_id::text = $1 ORDER BY updated_at, id",
    );
    let reading_progress: Vec<ReadingProgressRow> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as::<_, ReadingProgressRow>(&sql)
                .bind(account_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as::<_, ReadingProgressRow>(&sql)
                .bind(account_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };

    let sql = db.sql(
        "SELECT id, pseud_id, work_id, reason FROM did_not_finish \
         WHERE account_id = ? ORDER BY id",
        "SELECT id::text AS id, pseud_id::text AS pseud_id, work_id::text AS work_id, reason \
         FROM did_not_finish WHERE account_id::text = $1 ORDER BY id",
    );
    let did_not_finish: Vec<DidNotFinishRow> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as::<_, DidNotFinishRow>(&sql)
                .bind(account_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as::<_, DidNotFinishRow>(&sql)
                .bind(account_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };

    Ok(SubjectData {
        account_id: account_id.to_string(),
        email,
        handles,
        bookmarks,
        reading_progress,
        did_not_finish,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Erasure planning
// ─────────────────────────────────────────────────────────────────────────────

/// Count what an erasure would destroy, without destroying anything.
pub async fn plan_erasure(db: &Database, account_id: &str) -> Result<ErasurePlan> {
    let sql = db.sql(
        "SELECT handle FROM pseuds WHERE account_id = ? ORDER BY created_at LIMIT 1",
        "SELECT handle FROM pseuds WHERE account_id::text = $1 ORDER BY created_at LIMIT 1",
    );
    let handle: Option<String> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar::<_, String>(&sql)
                .bind(account_id)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar::<_, String>(&sql)
                .bind(account_id)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };

    let mut private_rows = Vec::new();
    for table in PRIVATE_TABLES {
        let sql = sql_owned(
            db,
            format!("SELECT COUNT(*) FROM {table} WHERE account_id = ?"),
            format!("SELECT COUNT(*) FROM {table} WHERE account_id::text = $1"),
        );
        let n: i64 = match db.backend() {
            Backend::Sqlite => {
                sqlx::query_scalar::<_, i64>(&sql)
                    .bind(account_id)
                    .fetch_one(db.sqlite_pool().expect("sqlite handle"))
                    .await?
            }
            Backend::Postgres => {
                sqlx::query_scalar::<_, i64>(&sql)
                    .bind(account_id)
                    .fetch_one(db.postgres_pool().expect("postgres handle"))
                    .await?
            }
        };
        private_rows.push((table.to_string(), n));
    }

    // The columns the cascade cannot reach. Counted per table rather than per column: a table with
    // two account columns (interaction_warmth, payment_events, trust_events) holds one row either
    // way, and a reader counting rows should see 1, not 2.
    let mut unlinked_rows: Vec<(String, i64)> = Vec::new();
    for column in FK_FREE_ACCOUNT_COLUMNS {
        if matches!(column.treatment, Treatment::DeferredTo(_)) {
            continue;
        }
        let sql = count_unlinked_sql(db, column);
        let n: i64 = match db.backend() {
            Backend::Sqlite => {
                sqlx::query_scalar::<_, i64>(&sql)
                    .bind(account_id)
                    .fetch_one(db.sqlite_pool().expect("sqlite handle"))
                    .await?
            }
            Backend::Postgres => {
                sqlx::query_scalar::<_, i64>(&sql)
                    .bind(account_id)
                    .fetch_one(db.postgres_pool().expect("postgres handle"))
                    .await?
            }
        };
        match unlinked_rows.iter_mut().find(|(t, _)| t == column.table) {
            Some((_, total)) => *total += n,
            None => unlinked_rows.push((column.table.to_string(), n)),
        }
    }

    // Published works are counted across ALL of the account's handles, not just the first. The
    // plan reports "the works you would lose", and a reader with three handles has three bodies
    // of work — counting only the first under-reports, and errs in the direction that destroys
    // someone's writing without warning.
    let sql = db.sql(
        "SELECT COUNT(*) FROM works WHERE owner_pseud_id IN \
             (SELECT id FROM pseuds WHERE account_id = ?) AND lifecycle = 'published'",
        "SELECT COUNT(*) FROM works WHERE owner_pseud_id IN \
             (SELECT id FROM pseuds WHERE account_id::text = $1) AND lifecycle = 'published'",
    );
    let published_work_count: i64 = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar::<_, i64>(&sql)
                .bind(account_id)
                .fetch_one(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar::<_, i64>(&sql)
                .bind(account_id)
                .fetch_one(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };

    // An export in flight would re-materialise the data moments after erasure.
    let sql = db.sql(
        "SELECT COUNT(*) FROM export_jobs WHERE account_id = ? \
           AND state IN ('queued', 'running')",
        "SELECT COUNT(*) FROM export_jobs WHERE account_id::text = $1 \
           AND state IN ('queued', 'running')",
    );
    let open_export_count: i64 = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar::<_, i64>(&sql)
                .bind(account_id)
                .fetch_one(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar::<_, i64>(&sql)
                .bind(account_id)
                .fetch_one(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };

    Ok(ErasurePlan {
        account_id: account_id.to_string(),
        handle,
        private_rows,
        unlinked_rows,
        published_work_count,
        open_export_count,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Erasure
// ─────────────────────────────────────────────────────────────────────────────

/// Erase `account_id` and everything hanging off it.
///
/// The deletion is one statement: the 71-table cascade, `api_tokens`' own `ON DELETE CASCADE`,
/// and `jobs.requested_by` going NULL are all the database's doing.
///
/// Open export jobs are cancelled first, deliberately. A job already running would assemble a
/// copy of the reader's data and hand it back through a download grant moments after the
/// account ceased to exist — an erasure that leaks via the export queue is not an erasure.
/// This is the one ordering in the module that is not obvious, so it is the one the spec's
/// mutation step reverts.
/// The `COUNT(*)` for one unlinked column, in the right shape for the engine and the column type.
fn count_unlinked_sql(db: &Database, column: &UnlinkedColumn) -> String {
    let (table, col) = (column.table, column.column);
    let sqlite = format!("SELECT COUNT(*) FROM {table} WHERE {col} = ?");
    let pg = match column.kind {
        ColumnKind::Text => format!("SELECT COUNT(*) FROM {table} WHERE {col} = $1"),
        ColumnKind::Uuid => format!("SELECT COUNT(*) FROM {table} WHERE {col}::text = $1"),
    };
    sql_owned(db, sqlite, pg)
}

/// The statement that applies one unlinked column's treatment.
///
/// `None` for a `DeferredTo` column: the erasure does not touch it yet, and returning a statement
/// whose failure would be swallowed is worse than returning nothing.
fn apply_unlinked_sql(db: &Database, column: &UnlinkedColumn) -> Option<String> {
    let (table, col) = (column.table, column.column);
    let predicate = |bound: &str| match column.kind {
        ColumnKind::Text => format!("{col} = {bound}"),
        // A `&str` bound to a `uuid` column is `incorrect binary data format`, so the comparison is
        // made in text on PostgreSQL. SQLite stores both as TEXT and needs no cast.
        ColumnKind::Uuid => match db.backend() {
            Backend::Postgres => format!("{col}::text = {bound}"),
            Backend::Sqlite => format!("{col} = {bound}"),
        },
    };
    match column.treatment {
        Treatment::Delete => Some(sql_owned(
            db,
            format!("DELETE FROM {table} WHERE {}", predicate("?")),
            format!("DELETE FROM {table} WHERE {}", predicate("$1")),
        )),
        Treatment::Sever => Some(sql_owned(
            db,
            format!("UPDATE {table} SET {col} = NULL WHERE {}", predicate("?")),
            format!("UPDATE {table} SET {col} = NULL WHERE {}", predicate("$1")),
        )),
        // `id` is the row's own primary key on every table in this list, and prefixing it makes the
        // value unlinkable while keeping it unique — which `UNIQUE (account_id, work_id)` requires.
        Treatment::Rekey => Some(sql_owned(
            db,
            format!(
                "UPDATE {table} SET {col} = 'erased:' || id WHERE {}",
                predicate("?")
            ),
            format!(
                "UPDATE {table} SET {col} = 'erased:' || id WHERE {}",
                predicate("$1")
            ),
        )),
        Treatment::DeferredTo(_) => None,
    }
}

pub async fn erase_account(db: &Database, account_id: &str) -> Result<()> {
    let cancel = db.sql(
        "UPDATE export_jobs SET state = 'cancelled' \
         WHERE account_id = ? AND state IN ('queued', 'running')",
        "UPDATE export_jobs SET state = 'cancelled' \
         WHERE account_id::text = $1 AND state IN ('queued', 'running')",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&cancel)
                .bind(account_id)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&cancel)
                .bind(account_id)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }

    let del = db.sql(
        "DELETE FROM accounts WHERE id = ?",
        "DELETE FROM accounts WHERE id::text = $1",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&del)
                .bind(account_id)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&del)
                .bind(account_id)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }

    // The cascade is finished; what it could not reach is here (M45-64). Every column in
    // `FK_FREE_ACCOUNT_COLUMNS` gets its treatment — the rows about this person are deleted, and the
    // rows that must survive (a payment record, an audit trail) lose their account link instead.
    //
    // After the account is gone, deliberately: if this pass fails, the account is already erased and
    // the spend is visible as an error the caller can retry — every statement here is idempotent, and
    // a half-erasure that reports success is the outcome worth avoiding.
    for column in FK_FREE_ACCOUNT_COLUMNS {
        let Some(sql) = apply_unlinked_sql(db, column) else {
            continue;
        };
        match db.backend() {
            Backend::Sqlite => {
                sqlx::query(&sql)
                    .bind(account_id)
                    .execute(db.sqlite_pool().expect("sqlite handle"))
                    .await?;
            }
            Backend::Postgres => {
                sqlx::query(&sql)
                    .bind(account_id)
                    .execute(db.postgres_pool().expect("postgres handle"))
                    .await?;
            }
        }
    }

    Ok(())
}
